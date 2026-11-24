//! No position drops silently (and the first review, I9). The reader keeps in the model what the model has slots for and counts
//! what it has not, and the file is the session's to keep (`diacraft_engine::file::OpenFile`). So whatever is *in* a part must end up in one of
//! two places after the read: in the model, where a re-emission shows it, or in the report's `skipped` count. **There is one place: the
//! model** (the report's skipped count is empty, and its slots hold what the model has no field for). This test makes the claim checkable.
//!
//! It takes a package that has every element the reader types, and for each element of each XML part it makes six files: one with a foreign
//! attribute on that element, one with a foreign child element, one with a comment, one with a processing instruction, one with a run of
//! text, and one with a namespace declaration, each marked with a word of its own. Each file is read. The marked thing must be in the
//! text `emit::everything` gives back from the model, **its extension slots included**, and nothing may be skipped.
//! A position where it is not is a silent drop, and is named by part, element and kind (never by content: the markers are fixed words).
//!
//! The first review found 122 of 252 injected positions absent from the model and 77 of those counted nowhere.

use diacraft_opc::Package;
use diacraft_opc::xml::{self, El, Node};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::{Limits, read};

use crate::emit::{everything, to_xml};
use crate::preservation::RICH_SHAPES;

/// A package with every kind of element the reader reads into a typed place, each once at least.
fn builder() -> VsdxBuilder {
    let master_part = format!(
        "<MasterContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes><Shape ID=\"4\" Type=\"Shape\"><Cell N=\"PinX\" V=\"1\" F=\"Width/2\"/><Text>m<cp IX=\"0\"/></Text></Shape><Shape ID=\"5\"/></Shapes><Connects><Connect FromSheet=\"4\" FromCell=\"BeginX\" FromPart=\"9\" ToSheet=\"5\" ToCell=\"PinX\" ToPart=\"3\"/></Connects></MasterContents>"
    );
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/><ColorEntry IX="25" RGB="#654321"/>"##)
        .face_names(r#"<FaceName NameU="Arial" Panose="1"/><FaceName NameU="Noto Sans"/>"#)
        .more_styles(
            r#"<StyleSheet ID="1" Name="Normal" NameU="Normal" LineStyle="0"><Cell N="LineWeight" V="0.01"/><Section N="Character"><Row IX="0"><Cell N="Size" V="1"/></Row></Section></StyleSheet>"#,
        )
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/><Section N="Reviewer"><Row IX="0"><Cell N="Name" V="x"/></Row></Section>"#)
        .document_extra(r#"<DocumentSettings TopPage="0"/><EventList/>"#)
        .master(MasterSpec::new(1, "M").icon("QUJD").part(&master_part))
        .master(MasterSpec::new(2, "N").shapes(r#"<Shape ID="4"/>"#))
        .page(
            PageSpec::new(1, "P")
                .sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/></Row></Section>"#)
                .shapes(RICH_SHAPES)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/image1.png"),
        )
        .page(PageSpec::new(2, "Q").without_part())
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", [1u8, 2, 3]))
}

/// The parts whose elements are injected into.
const PARTS: &[&str] = &[
    "/visio/document.xml",
    "/visio/pages/pages.xml",
    "/visio/masters/masters.xml",
    "/visio/pages/page1.xml",
    "/visio/masters/master1.xml",
    // The property parts: the elements whose text a field of the properties reads hold what they have beside it, too (`VsdxHeld::props_rest`).
    "/docProps/core.xml",
    "/docProps/app.xml",
];

#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Attribute,
    Element,
    Comment,
    Instruction,
    Text,
    Declaration,
}

impl Kind {
    pub(crate) const ALL: [Kind; 6] = [Kind::Attribute, Kind::Element, Kind::Comment, Kind::Instruction, Kind::Text, Kind::Declaration];

    /// The word the injection is marked with, and which the emission must hold if the model does.
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Kind::Attribute => "zzqa",
            Kind::Element => "zzqe",
            Kind::Comment => "zzqc",
            Kind::Instruction => "zzqp",
            Kind::Text => "zzqt",
            Kind::Declaration => "zzqx",
        }
    }

    pub(crate) fn inject(self, el: &mut El) {
        match self {
            Kind::Attribute => el.attrs.push(("zzqa".to_owned(), "1".to_owned())),
            Kind::Element => el
                .children
                .insert(0, Node::El(El { name: "q:zzqe".to_owned(), attrs: vec![("xmlns:q".to_owned(), "urn:zzq".to_owned())], children: vec![] })),
            Kind::Comment => el.children.insert(0, Node::Comment("zzqc".to_owned())),
            Kind::Instruction => el.children.insert(0, Node::Pi { target: "zzqp".to_owned(), data: "d".to_owned() }),
            Kind::Text => el.children.insert(0, Node::Text("zzqt".to_owned())),
            Kind::Declaration => el.attrs.push(("xmlns:zzqx".to_owned(), "urn:zzqx".to_owned())),
        }
    }
}

/// The element at preorder number `target`, to change; counts as it goes. No recursion deeper than the tree.
fn at(el: &mut El, next: &mut usize, target: usize, f: &mut dyn FnMut(&mut El)) -> bool {
    if *next == target {
        f(el);
        return true;
    }
    *next += 1;
    for child in &mut el.children {
        if let Node::El(c) = child
            && at(c, next, target, f)
        {
            return true;
        }
    }
    false
}

fn elements(el: &El) -> usize {
    1 + el.children.iter().map(|c| if let Node::El(c) = c { elements(c) } else { 0 }).sum::<usize>()
}

fn name_at(el: &El, next: &mut usize, target: usize) -> Option<String> {
    if *next == target {
        return Some(el.name.clone());
    }
    *next += 1;
    el.children.iter().find_map(|c| if let Node::El(c) = c { name_at(c, next, target) } else { None })
}

fn skipped_total(report: &diacraft_vsdx::ReadReport) -> u64 {
    report.skipped_counts.values().sum()
}

#[test]
fn no_position_drops_silently() {
    let builder = builder();
    let original = builder.pkg().build();
    let (baseline_doc, baseline) = read(&original, &Limits::default()).unwrap_or_else(|e| panic!("the base package must read: {e}"));
    // The base is a fixed point of this test: what it holds and counts without any injection.
    let baseline_total = skipped_total(&baseline);
    let held = everything(&baseline_doc);
    assert!(held.contains("Panose"), "the emission holds the face names: {held:.200}");
    let package = Package::read(&original, &diacraft_opc::Limits::default()).unwrap();
    let mut silent: Vec<String> = Vec::new();
    let mut positions = 0usize;
    for part in PARTS {
        let tree = xml::parse(package.part(part).unwrap_or_else(|| panic!("the base has no part {part}"))).unwrap();
        for index in 0..elements(&tree) {
            let element = name_at(&tree, &mut 0, index).unwrap();
            for kind in Kind::ALL {
                positions += 1;
                let mut mutated = tree.clone();
                at(&mut mutated, &mut 0, index, &mut |e| kind.inject(e));
                let text = to_xml(&mutated);
                let bytes = builder
                    .pkg()
                    .map_parts(|name, bytes| {
                        if name.eq_ignore_ascii_case(part) {
                            *bytes = text.clone().into_bytes();
                        }
                    })
                    .build();
                match read(&bytes, &Limits::default()) {
                    Err(e) => silent.push(format!("{part} <{element}> #{index} {kind:?}: the file was refused ({})", e.class())),
                    Ok((doc, report)) => {
                        let in_model = everything(&doc).contains(kind.marker());
                        if !in_model || skipped_total(&report) != baseline_total {
                            silent.push(format!("{part} <{element}> #{index} {kind:?}"));
                        }
                    }
                }
            }
        }
    }
    eprintln!("[drops] {positions} positions injected into {} parts; {} silent", PARTS.len(), silent.len());
    assert!(positions > 200, "the base package must have a few hundred positions: {positions}");
    assert!(silent.is_empty(), "{} positions drop silently:\n{}", silent.len(), silent.join("\n"));
}
