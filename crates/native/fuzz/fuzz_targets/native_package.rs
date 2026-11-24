//! `cargo fuzz run native_package`: a valid `.diacraft` package written from a seed model, with the entry that the first byte picks replaced by the rest of the input (and kept
//! under the same name and method), so that the fuzzer explores the reader past the archive: the manifest, the document part, a page, a blob, the state, a part of an unknown role.
//! The properties are those of `native_read`: an error in words, or a model within the caps that writes (or is refused for a limit), reads back equal, and the write after it
//! is a fixed point. The package is read once as it is made, whole, before anything is replaced (a seed that does not read is a crash of the target itself).
#![no_main]

use std::sync::Arc;

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_container::{Limits as ContainerLimits, Work};
use diacraft_doc::{Document, ForeignData, Media, Page, Part, PartRole, RawAttrs, Shape, ShapeList, ShapeType};
use diacraft_native::{Limits, State, WriteOptions, read, write};
use libfuzzer_sys::fuzz_target;

/// The seed: a drawing of two pages that show one picture and one of another, a held package part, and a part of an unknown role from a newer minor version.
fn seed() -> Vec<u8> {
    let picture = |tag: u8| -> Arc<[u8]> { Arc::from([&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[tag; 24][..]].concat()) };
    let shape = |id: u32, bytes: &Arc<[u8]>| {
        let media = Media { part: Arc::from("media/a.png"), content_type: Arc::from("image/png"), bytes: Arc::clone(bytes) };
        let foreign =
            ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]), picture: Some(Arc::new(media)), ..ForeignData::default() };
        Arc::new(Shape { id, kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() })
    };
    let (a, b) = (picture(1), picture(2));
    let mut doc = Document::new_blank();
    let mut one = Page::new(1, "One");
    one.shapes = ShapeList::from_vec(vec![shape(1, &a), shape(2, &a)]);
    let mut two = Page::new(2, "Two");
    two.shapes = ShapeList::from_vec(vec![shape(1, &b)]);
    doc.pages = diacraft_doc::SharedVec::from_vec(vec![Arc::new(one), Arc::new(two)]);
    let held = Part {
        format: diacraft_doc::Format::Vsdx,
        name: Arc::from("/e/object.bin"),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Embedding,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(vec![7u8; 40]),
    };
    let extra = Part {
        format: diacraft_doc::Format::Native,
        name: Arc::from("notes/review.txt"),
        media_type: Arc::from("text/plain"),
        role: PartRole::Other,
        manifest: RawAttrs::from_pairs([("role", "annotation")]),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(b"check the margins".to_vec()),
    };
    doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(held), Arc::new(extra)]);
    // With the picture of the first page (GATE.b94e.F1): a save of a package that changed nothing copies it.
    let preview = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"first page"].concat();
    write(&doc, &State::default(), None, &WriteOptions { preview: Some(preview), ..WriteOptions::default() }).expect("the seed is written").bytes
}

/// The namespace the builds before the id moved to `diacraft.app` wrote, and the one now (APPID).
const BEFORE: &str = "urn:io.github.one_bit.diacraft:extension";
const NOW: &str = "urn:app.diacraft:extension";

fn small() -> Limits {
    let mut limits = Limits { max_file_bytes: 4 << 20, max_memory: 256 << 20, ..Limits::default() };
    limits.container.max_package_bytes = 2 << 20;
    limits.container.xml.max_nodes = 200_000;
    limits
}

fuzz_target!(|data: &[u8]| {
    let Some((first, rest)) = data.split_first() else { return };
    let seed = seed();
    let mut archive = Archive::read(&seed, &ContainerLimits::default()).expect("the seed is a package");
    let count = archive.entries().len();
    // The high bit of the first byte puts every XML entry of the seed in the namespace of the builds before the id moved (GATE.b94e.F1: the document part of such a file is no longer the bytes a save
    // makes, and its preview is copied still).
    let before = *first & 0x80 != 0;
    let target = usize::from(*first & 0x7f) % count;
    let mut work = Work::default();
    let mut out = ArchiveWriter::new();
    for index in 0..count {
        let (name, method) = {
            let entry = &archive.entries()[index];
            (entry.name.clone(), entry.compression().unwrap_or(Method::Deflated))
        };
        let content = archive.inflate(index, &mut work).expect("an entry of the seed inflates");
        let spelled: Vec<u8> = match String::from_utf8(content) {
            Ok(text) if before && name.ends_with(".xml") => text.replace(NOW, BEFORE).into_bytes(),
            Ok(text) => text.into_bytes(),
            Err(bytes) => bytes.into_bytes(),
        };
        let content: &[u8] = if index == target { rest } else { &spelled };
        out.add(&name, content, method).expect("an entry is added");
    }
    let bytes = out.finish().expect("the package is finished");
    let limits = small();
    let (doc, opened) = match read(&bytes, &limits) {
        Ok(read) => read,
        Err(e) => {
            assert!(!e.to_string().is_empty() && !e.class().is_empty(), "a refusal has words and a class");
            return;
        }
    };
    assert!(doc.check_limits().is_ok(), "a model that was read holds the model's caps");
    let options = WriteOptions { limits: limits.clone(), ..WriteOptions::default() };
    // Copied from the package as it was read, whichever namespace it is in: it reads back as the model, and a picture it has is the picture the package had.
    if let Ok(saved) = write(&doc, &opened.state, Some(&diacraft_native::Previous { bytes: &bytes, decoded: &opened.decoded }), &options) {
        let Ok((model, _)) = read(&saved.bytes, &limits) else { panic!("a package made from the one read is read") };
        assert!(model == doc, "a package made from the one read reads back as the model");
    }
    let Ok(first) = write(&doc, &opened.state, None, &options) else { return };
    let Ok((back, again)) = read(&first.bytes, &limits) else { return };
    assert!(back == doc, "a model read, written and read is the model");
    // Copied from what the read learned, a write is a package that reads back as the model it was made from: a part is copied only when it still means what it meant.
    if let Ok(copied) = write(&back, &again.state, Some(&diacraft_native::Previous { bytes: &first.bytes, decoded: &again.decoded }), &options) {
        let Ok((model, _)) = read(&copied.bytes, &limits) else { panic!("a package made by copying is read") };
        assert!(model == doc, "a package made by copying reads back as the model");
    }
});
