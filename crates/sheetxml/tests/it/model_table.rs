//! The vocabulary table against the decoder: for every attribute of every element that has any, the decoder refuses an element
//! without it **exactly when the table says it is required**, and reads an element with only the optional ones it is given. The public spec copies the table, so
//! a table that calls an attribute optional or defaulted where the decoder refuses its absence is a misstatement of the format.
use diacraft_container::xml::{El, parse_with};
use diacraft_sheetxml::model::vocab::{AttrDef, ELEMENTS, Kind};
use diacraft_sheetxml::model::{BlobId, BlobSource, Budget, CodecError, URN, decode, part_lists};
use std::sync::Arc;

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

/// The one blob the tests name: a byte `x`, number 0.
pub(super) struct OneBlob;

impl BlobSource for OneBlob {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        (id.0 == 0).then(|| Arc::from(&b"x"[..]))
    }
}

/// Which part an element is tested in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Root {
    Document,
    Page,
    Master,
}

pub(super) fn dx(s: &str) -> String {
    s.replace("xmlns:QD=", "xmlns:diacraft=").replace("QD:", "diacraft:")
}

pub(super) fn parse(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap_or_else(|e| panic!("{e}: {xml}"))
}

/// The record of the blob a theme, a picture or a part can name: the record of [`OneBlob`]'s bytes, so that what names it is built and not lost.
fn blob_record() -> String {
    format!(
        "<QD:blob id=\"0\" path=\"blobs/0.bin\" media-type=\"a/b\" size=\"1\" stamp=\"{}\"/>",
        diacraft_sheetxml::model::stamp_of_bytes(b"x").text()
    )
}

/// The text of the element `name` with `attrs`, in the part where it stands, and the part: the document part when it is the document's, else a page or a master.
pub(super) fn context(name: &str, attrs: &str) -> Option<(Root, String)> {
    let blob_record = blob_record();
    let doc = |inner: &str| (Root::Document, format!("<QD:document xmlns:QD=\"{URN}\">{blob_record}{inner}</QD:document>"));
    let page = |inner: &str| (Root::Page, format!("<QD:page xmlns:QD=\"{URN}\">{inner}</QD:page>"));
    let shape = |inner: &str| page(&format!("<QD:shape id=\"1\">{inner}</QD:shape>"));
    let text = |inner: &str| shape(&format!("<QD:text>{inner}</QD:text>"));
    let in_sheet = |inner: &str| page(&format!("<QD:sheet>{inner}</QD:sheet>"));
    let in_text_style = |inner: &str| {
        shape(&format!("<QD:text><QD:ext><QD:odf><QD:paragraph><QD:text-style>{inner}</QD:text-style></QD:paragraph></QD:odf></QD:ext></QD:text>"))
    };
    let in_odf = |inner: &str| doc(&format!("<QD:ext><QD:odf>{inner}</QD:odf></QD:ext>"));
    let el_child = "<QD:el name=\"x\"/>";
    let out = match name {
        "document" => (Root::Document, format!("<QD:document xmlns:QD=\"{URN}\" {attrs}>{blob_record}</QD:document>")),
        "page-ref" => doc(&format!("<QD:page-ref {attrs}/>")),
        "master-ref" => doc(&format!("<QD:master-ref {attrs}/>")),
        "blob" => (Root::Document, format!("<QD:document xmlns:QD=\"{URN}\"><QD:blob {attrs}/></QD:document>")),
        "props" => doc(&format!("<QD:props {attrs}/>")),
        "custom" => doc(&format!("<QD:props><QD:custom {attrs}/></QD:props>")),
        "comment-list" => doc(&format!("<QD:comment-list {attrs}/>")),
        "comment-author" => doc(&format!("<QD:comment-list><QD:comment-author {attrs}/></QD:comment-list>")),
        "comment-entry" => doc(&format!("<QD:comment-list><QD:comment-entry {attrs}/></QD:comment-list>")),
        "color" => doc(&format!("<QD:color {attrs}/>")),
        "font" => doc(&format!("<QD:font {attrs}/>")),
        "style" => doc(&format!("<QD:style {attrs}/>")),
        "theme" => doc(&format!("<QD:theme {attrs}/>")),
        "part" => doc(&format!("<QD:part {attrs}/>")),
        "rel" => doc(&format!("<QD:vsdx-held><QD:rel {attrs}/></QD:vsdx-held>")),
        "content-type" => doc(&format!("<QD:vsdx-held><QD:content-type {attrs}/></QD:vsdx-held>")),
        "rest" => doc(&format!("<QD:ext><QD:vsdx><QD:rest {attrs}/></QD:vsdx></QD:ext>")),
        "odf" => doc(&format!("<QD:ext><QD:odf {attrs}/></QD:ext>")),
        "root" => in_odf(&format!("<QD:root {attrs}/>")),
        "manifest-row" => in_odf(&format!("<QD:manifest-row {attrs}/>")),
        "definition" => in_odf(&format!("<QD:definition {attrs}>{el_child}</QD:definition>")),
        "kept-style" => in_odf(&format!("<QD:kept-style {attrs}>{el_child}</QD:kept-style>")),
        "layer" => in_odf(&format!("<QD:layer {attrs}>{el_child}</QD:layer>")),
        "odf-raw" => text(&format!("<QD:odf-raw {attrs}><QD:txt>YQ==</QD:txt></QD:odf-raw>")),
        "page" => (Root::Page, format!("<QD:page xmlns:QD=\"{URN}\" {attrs}/>")),
        "master" => (Root::Master, format!("<QD:master xmlns:QD=\"{URN}\" {attrs}/>")),
        "shape" => page(&format!("<QD:shape {attrs}/>")),
        "connect" => page(&format!("<QD:connect {attrs}/>")),
        "sheet" => page(&format!("<QD:sheet {attrs}/>")),
        // A `same` names a sheet that an element before it gave a name: the sample of a text is `x`.
        "same" => page(&format!("<QD:shape id=\"1\"><QD:sheet def=\"x\"/></QD:shape><QD:shape id=\"2\"><QD:same {attrs}/></QD:shape>")),
        "Cell" => in_sheet(&format!("<QD:Cell {attrs}/>")),
        "extra" => in_sheet(&format!("<QD:Cell N=\"A\"><QD:extra {attrs}/></QD:Cell>")),
        "Section" => in_sheet(&format!("<QD:Section {attrs}/>")),
        "Row" => in_sheet(&format!("<QD:Section N=\"User\"><QD:Row {attrs}/></QD:Section>")),
        "attr" => shape(&format!("<QD:attr {attrs}/>")),
        "kept" => shape(&format!("<QD:kept {attrs}><QD:txt>YQ==</QD:txt></QD:kept>")),
        "el" => shape(&format!("<QD:kept><QD:el {attrs}/></QD:kept>")),
        "txt" => shape(&format!("<QD:kept><QD:txt {attrs}>YQ==</QD:txt></QD:kept>")),
        "comment" => shape(&format!("<QD:kept><QD:comment {attrs}>YQ==</QD:comment></QD:kept>")),
        "pi" => shape(&format!("<QD:kept><QD:pi {attrs}>YQ==</QD:pi></QD:kept>")),
        "s" => shape(&format!("<QD:s {attrs}>YQ==</QD:s>")),
        "foreign" => shape(&format!("<QD:foreign {attrs}/>")),
        "picture" => shape(&format!("<QD:foreign><QD:picture {attrs}/></QD:foreign>")),
        "t" => text(&format!("<QD:t {attrs}>YQ==</QD:t>")),
        "cp" => text(&format!("<QD:cp {attrs}/>")),
        "pp" => text(&format!("<QD:pp {attrs}/>")),
        "tp" => text(&format!("<QD:tp {attrs}/>")),
        "fld" => text(&format!("<QD:fld {attrs}>YQ==</QD:fld>")),
        "held" => shape(&format!("<QD:ext><QD:odf><QD:held {attrs}/></QD:odf></QD:ext>")),
        "paragraph" => shape(&format!("<QD:text><QD:ext><QD:odf><QD:paragraph {attrs}/></QD:odf></QD:ext></QD:text>")),
        "run" => shape(&format!("<QD:text><QD:ext><QD:odf><QD:paragraph><QD:run {attrs}/></QD:paragraph></QD:odf></QD:ext></QD:text>")),
        "text-style" => shape(&format!("<QD:text><QD:ext><QD:odf><QD:paragraph><QD:text-style {attrs}/></QD:paragraph></QD:odf></QD:ext></QD:text>")),
        "held-style" => in_text_style(&format!("<QD:held-style {attrs}/>")),
        _ => return None,
    };
    Some((out.0, dx(&out.1)))
}

/// What a test gives an attribute that it has to give one.
pub(super) fn sample(element: &str, a: &AttrDef) -> String {
    match (element, a.name) {
        ("part", "format") | ("custom", "format") => "vsdx",
        ("part", "role") => "thumbnail",
        ("definition", "family") => "marker",
        ("held", "group") => "fill",
        ("held-style", "group") => diacraft_doc::TextGroup::ALL[0].id(),
        ("shape", "type") => "Shape",
        // The ID the document's list gives a page or a master with no ID of its own.
        ("page" | "master", "id") => "0",
        _ => match a.kind {
            Kind::Text => "x",
            Kind::U32 | Kind::U64 | Kind::Blob => "0",
            Kind::Flag => "1",
            Kind::Stamp => "0123456789abcdef",
        },
    }
    .to_owned()
}

/// What the attributes of an element are, as text: the ones given, each with its sample.
pub(super) fn attrs_text(element: &str, names: &[&AttrDef]) -> String {
    names.iter().map(|a| format!("{}=\"{}\"", a.name, sample(element, a))).collect::<Vec<_>>().join(" ")
}

/// The trees of a document and the parts it names: what a decode is given.
pub(super) struct Prepared {
    pub document: El,
    pub pages: Vec<El>,
    pub masters: Vec<El>,
}

/// Parses the part `text` and the document it belongs to (the document itself when it is one), and an empty page or master of the ID the list gives for every
/// other part the list names.
pub(super) fn prepare(root: Root, text: &str) -> Result<Prepared, CodecError> {
    let blob_record = blob_record();
    let document_text = match root {
        Root::Document => text.to_owned(),
        Root::Page => dx(&format!("<QD:document xmlns:QD=\"{URN}\">{blob_record}<QD:page-ref/></QD:document>")),
        Root::Master => dx(&format!("<QD:document xmlns:QD=\"{URN}\">{blob_record}<QD:master-ref/></QD:document>")),
    };
    let document = parse(&document_text);
    let lists = part_lists(&document)?;
    let empty = |local: &str, id: u32| parse(&dx(&format!("<QD:{local} xmlns:QD=\"{URN}\" id=\"{id}\"/>")));
    let mut pages: Vec<El> = lists.pages.iter().map(|(id, _)| empty("page", *id)).collect();
    let mut masters: Vec<El> = lists.masters.iter().map(|(id, _)| empty("master", *id)).collect();
    match root {
        Root::Page => pages = vec![parse(text)],
        Root::Master => masters = vec![parse(text)],
        Root::Document => {}
    }
    Ok(Prepared { document, pages, masters })
}

/// Decodes what [`prepare`] made, within `budget`.
pub(super) fn run(prepared: &Prepared, budget: &mut Budget) -> Result<(), CodecError> {
    let (p, m): (Vec<&El>, Vec<&El>) = (prepared.pages.iter().collect(), prepared.masters.iter().collect());
    decode(&prepared.document, &p, &m, &OneBlob, budget, &mut Vec::new()).map(|_| ())
}

/// Decodes the part `text` in the document the part belongs to, or the document itself.
fn decode_in(root: Root, text: &str) -> Result<(), CodecError> {
    run(&prepare(root, text)?, &mut Budget::new(HUGE, HUGE, HUGE))
}

fn refused(out: &Result<(), CodecError>) -> bool {
    matches!(out, Err(CodecError::Malformed { .. }))
}

#[test]
fn the_decoder_refuses_exactly_the_required_absences_the_table_names_and_only_those() {
    let mut untested = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for def in ELEMENTS.iter().filter(|e| !e.attrs.is_empty()) {
        if context(def.name, "").is_none() {
            untested.push(def.name);
            continue;
        }
        let required: Vec<&AttrDef> = def.attrs.iter().filter(|a| a.required).collect();
        // A table that says required says no default.
        for a in &required {
            if a.default.is_some() {
                problems.push(format!("{}@{} is required and has a default", def.name, a.name));
            }
        }
        // The base: the required attributes, and (for a content-type row, which needs exactly one of two) the one of them it takes.
        let base = |given: &[&AttrDef]| {
            let one_of = def.name == "content-type" && !given.iter().any(|a| a.name == "extension" || a.name == "part");
            let extra = if one_of { " extension=\"x\"" } else { "" };
            let (root, text) = context(def.name, &format!("{}{extra}", attrs_text(def.name, given))).unwrap();
            decode_in(root, &text)
        };
        let out = base(&required);
        if out.is_err() {
            problems.push(format!("{} with its required attributes only: {:?}", def.name, out.err()));
        }
        checked += 1;
        for skip in &required {
            let given: Vec<&AttrDef> = required.iter().copied().filter(|a| a.name != skip.name).collect();
            let out = base(&given);
            if !refused(&out) {
                problems.push(format!("{} without the required {} is not refused: {:?}", def.name, skip.name, out));
            }
            checked += 1;
        }
        for optional in def.attrs.iter().filter(|a| !a.required) {
            let mut given = required.clone();
            given.push(optional);
            let out = base(&given);
            if out.is_err() {
                problems.push(format!("{} with the optional {} too: {:?}", def.name, optional.name, out.err()));
            }
            // And without it, which is the base: an absent optional attribute is read as its default, or as none, and never refused.
            checked += 1;
        }
    }
    assert!(untested.is_empty(), "elements with attributes that no context here places: {untested:?}");
    assert!(problems.is_empty(), "the table and the decoder disagree:\n{}", problems.join("\n"));
    assert!(checked > 100, "{checked} cases");
}

#[test]
fn a_content_type_row_takes_exactly_one_of_an_extension_and_a_part() {
    let row = |attrs: &str| {
        let (root, text) = context("content-type", attrs).unwrap();
        decode_in(root, &text)
    };
    assert!(row("extension=\"x\" content-type=\"t\"").is_ok(), "a default");
    assert!(row("part=\"/x\" content-type=\"t\"").is_ok(), "an override");
    assert!(refused(&row("content-type=\"t\"")), "neither");
    assert!(refused(&row("extension=\"x\" part=\"/x\" content-type=\"t\"")), "both");
}

// ------------------------------------------------------------------------------------------------------------------------------------- the table's words

#[test]
fn every_element_that_ext_stands_in_says_in_its_holds_that_it_holds_one() {
    // The public spec copies the table, and the `holds` of an element that an `ext` stands in (a style, a font, a sheet, a connect, ...) left it out
    // where the encoder writes it. The elements the `ext` names as its places are the elements whose words must name it.
    let ext = diacraft_sheetxml::model::vocab::element(diacraft_sheetxml::model::vocab::el::EXT).expect("the ext element");
    assert!(ext.inside.len() >= 13, "{:?}", ext.inside);
    for owner in ext.inside {
        let def = diacraft_sheetxml::model::vocab::element(owner).unwrap_or_else(|| panic!("{owner} is an element of the table"));
        let words: Vec<&str> = def.holds.split(|c: char| !c.is_alphanumeric()).collect();
        assert!(words.contains(&"ext"), "the words of {owner} do not name the ext it holds: {}", def.holds);
    }
}

#[test]
fn the_local_names_of_the_table_are_each_in_it_once() {
    let mut seen = std::collections::HashSet::new();
    for def in diacraft_sheetxml::model::vocab::ELEMENTS {
        assert!(seen.insert(def.name), "{} is in the table twice", def.name);
    }
}
