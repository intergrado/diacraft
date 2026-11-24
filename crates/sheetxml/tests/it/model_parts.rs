//! Part B: **a decode and a save hold one part's tree at a time.** An open asks its source for a part, reads it, lets the tree go, and asks for the
//! next ([`PartSource`]); a save makes a part, checks it as an open would read it, hands it over and lets it go ([`save_parts`]). The first figures of the open
//! budget were sized for every part's tree held at once.
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, XmlLimits, parse_with, to_bytes};
use diacraft_doc::{Document, Master, Page, Shape, ShapeList};
use diacraft_sheetxml::model::{
    Budget, CodecError, MemBlobs, PartRole, PartSource, URN, check_decodes_from, decode, decode_from, encode_for_save, open_limits, save_parts,
};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;

const HUGE: u64 = 1 << 40;

fn malformed(part: &str, why: &'static str) -> CodecError {
    CodecError::Malformed { part: part.to_owned(), why }
}

// ------------------------------------------------------------------------------------------------------------------------------- a source that counts

/// A tree that counts itself while it is alive.
struct Tracked {
    el: El,
    live: Rc<Cell<usize>>,
}

impl std::borrow::Borrow<El> for Tracked {
    fn borrow(&self) -> &El {
        &self.el
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}

/// A source that makes each part when it is asked for it, keeps none of them, and says what it was asked and how many trees were alive at once.
struct Lazy {
    pages: Vec<El>,
    masters: Vec<El>,
    live: Rc<Cell<usize>>,
    most_live: usize,
    asked: Vec<String>,
    fail_at: Option<usize>,
}

impl Lazy {
    fn new(pages: Vec<El>, masters: Vec<El>) -> Lazy {
        Lazy { pages, masters, live: Rc::new(Cell::new(0)), most_live: 0, asked: Vec::new(), fail_at: None }
    }

    fn hand_over(&mut self, what: &str, el: Option<&El>) -> Result<Tracked, CodecError> {
        self.asked.push(what.to_owned());
        if self.fail_at == Some(self.asked.len()) {
            return Err(malformed("page", "a part the source could not read"));
        }
        let el = el.cloned().ok_or_else(|| malformed("document", "no such part"))?;
        self.live.set(self.live.get() + 1);
        self.most_live = self.most_live.max(self.live.get());
        Ok(Tracked { el, live: Rc::clone(&self.live) })
    }
}

impl PartSource for Lazy {
    type Tree = Tracked;

    fn page(&mut self, index: usize, name: &str) -> Result<Tracked, CodecError> {
        let el = self.pages.get(index).cloned();
        self.hand_over(&format!("page {index} {name}"), el.as_ref())
    }

    fn master(&mut self, index: usize, name: &str) -> Result<Tracked, CodecError> {
        let el = self.masters.get(index).cloned();
        self.hand_over(&format!("master {index} {name}"), el.as_ref())
    }
}

fn shape(id: u32) -> Arc<Shape> {
    Arc::new(Shape::new(id))
}

fn page(id: u32, shapes: u32) -> Arc<Page> {
    let mut page = Page::new(id, &format!("page {id}"));
    page.shapes = ShapeList::from_vec((0..shapes).map(shape).collect());
    Arc::new(page)
}

/// A drawing of `pages` pages and `masters` masters, each of a few shapes.
fn drawing(pages: u32, masters: u32) -> Document {
    let mut doc = Document::empty();
    for p in 0..pages {
        doc.pages.push(page(p + 1, 3 + p));
    }
    for m in 0..masters {
        doc.masters.push(Arc::new(Master { id: m + 10, shapes: ShapeList::from_vec(vec![shape(1)]), ..Master::default() }));
    }
    doc
}

fn refs(v: &[El]) -> Vec<&El> {
    v.iter().collect()
}

fn lazy_of(doc: &Document) -> (El, Lazy, MemBlobs) {
    let mut blobs = MemBlobs::new();
    let encoded = encode_for_save(doc, &mut blobs, &open_limits()).unwrap();
    let pages = encoded.pages.into_iter().map(|(_, e)| e).collect();
    let masters = encoded.masters.into_iter().map(|(_, e)| e).collect();
    (encoded.document, Lazy::new(pages, masters), blobs)
}

// ------------------------------------------------------------------------------------------------------------------------------- an open

#[test]
fn a_decode_asks_for_each_part_once_in_order_and_holds_one_tree_at_a_time() {
    let doc = drawing(5, 3);
    let (document, mut source, blobs) = lazy_of(&doc);
    let back = decode_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, doc);
    assert_eq!(
        source.asked,
        [
            "page 0 pages/page1.xml",
            "page 1 pages/page2.xml",
            "page 2 pages/page3.xml",
            "page 3 pages/page4.xml",
            "page 4 pages/page5.xml",
            "master 0 masters/master10.xml",
            "master 1 masters/master11.xml",
            "master 2 masters/master12.xml",
        ]
    );
    assert_eq!(source.most_live, 1, "a tree is let go before the next is asked for");
    assert_eq!(source.live.get(), 0, "and none is held once the decode is done");
}

#[test]
fn a_check_that_keeps_nothing_holds_one_tree_at_a_time_too() {
    let doc = drawing(4, 2);
    let (document, mut source, blobs) = lazy_of(&doc);
    check_decodes_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(source.asked.len(), 6);
    assert_eq!(source.most_live, 1);
    assert_eq!(source.live.get(), 0);
}

#[test]
fn a_source_that_gives_its_parts_one_at_a_time_gives_what_a_caller_that_holds_them_all_does() {
    let doc = drawing(3, 1);
    let (document, mut source, blobs) = lazy_of(&doc);
    let (pages, masters) = (source.pages.clone(), source.masters.clone());
    let (mut a, mut b) = (Budget::new(HUGE, HUGE, HUGE), Budget::new(HUGE, HUGE, HUGE));
    let one_by_one = decode_from(&document, &mut source, &blobs, &mut a, &mut Vec::new()).unwrap();
    let (page_refs, master_refs): (Vec<&El>, Vec<&El>) = (pages.iter().collect(), masters.iter().collect());
    let all_at_once = decode(&document, &page_refs, &master_refs, &blobs, &mut b, &mut Vec::new()).unwrap();
    assert_eq!(one_by_one, all_at_once);
    assert_eq!((a.items, a.bytes, a.blob_bytes), (b.items, b.bytes, b.blob_bytes));
}

#[test]
fn a_part_the_source_cannot_give_ends_the_decode_with_its_error_and_no_later_part_is_asked_for() {
    let doc = drawing(4, 1);
    let (document, mut source, blobs) = lazy_of(&doc);
    source.fail_at = Some(2);
    let refused = decode_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap_err();
    assert_eq!(refused, malformed("page", "a part the source could not read"));
    assert_eq!(source.asked.len(), 2, "{:?}", source.asked);
}

#[test]
fn a_page_part_whose_id_is_not_the_lists_is_damage_as_it_was() {
    let doc = drawing(2, 0);
    let (document, mut source, blobs) = lazy_of(&doc);
    source.pages.swap(0, 1);
    let refused = decode_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap_err();
    assert_eq!(refused, malformed("page", "a page part whose ID is not the one the list gives"));
    // And a master's.
    let doc = drawing(0, 2);
    let (document, mut source, blobs) = lazy_of(&doc);
    source.masters.swap(0, 1);
    let refused = decode_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap_err();
    assert_eq!(refused, malformed("master", "a master part whose ID is not the one the list gives"));
}

#[test]
fn a_caller_that_holds_the_parts_is_told_when_the_lists_name_others_before_anything_is_read() {
    let doc = drawing(2, 1);
    let (document, source, blobs) = lazy_of(&doc);
    let (pages, masters) = (refs(&source.pages), refs(&source.masters));
    let wrong = malformed("document", "the lists name other parts than were given");
    let attempt = |pages: &[&El], masters: &[&El]| decode(&document, pages, masters, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new());
    assert!(attempt(&pages, &masters).is_ok());
    assert_eq!(attempt(&pages[..1], &masters).unwrap_err(), wrong, "a page short");
    assert_eq!(attempt(&pages, &[]).unwrap_err(), wrong, "a master short");
    assert_eq!(attempt(&[pages[0], pages[1], pages[0]], &masters).unwrap_err(), wrong, "a page too many");
    assert_eq!(attempt(&pages, &[masters[0], masters[0]]).unwrap_err(), wrong, "a master too many");
}

// ------------------------------------------------------------------------------------------------------------------------------- a save

/// What a save hands over, in order: the role and name of each part, and its tree.
/// What a save handed over: how it ended, each part (its role, its name and its tree), and the blobs.
type Saved = (Result<(), CodecError>, Vec<(PartRole, String, El)>, MemBlobs);

fn saved(doc: &Document, limits: &Limits) -> Saved {
    let mut blobs = MemBlobs::new();
    let mut parts = Vec::new();
    let out = save_parts(doc, &mut blobs, limits, &mut |p| {
        parts.push((p.role, p.name.to_owned(), p.tree.clone()));
        Ok(())
    });
    (out, parts, blobs)
}

#[test]
fn a_save_hands_over_the_document_then_each_page_then_each_master_under_the_names_the_document_lists() {
    let doc = drawing(3, 2);
    let (out, parts, _) = saved(&doc, &open_limits());
    out.unwrap();
    let roles: Vec<(PartRole, &str)> = parts.iter().map(|(r, n, _)| (*r, n.as_str())).collect();
    assert_eq!(
        roles,
        [
            (PartRole::Document, ""),
            (PartRole::Page { id: 1 }, "pages/page1.xml"),
            (PartRole::Page { id: 2 }, "pages/page2.xml"),
            (PartRole::Page { id: 3 }, "pages/page3.xml"),
            (PartRole::Master { id: 10 }, "masters/master10.xml"),
            (PartRole::Master { id: 11 }, "masters/master11.xml"),
        ]
    );
}

#[test]
fn what_a_save_hands_over_is_what_the_save_that_holds_every_part_returns() {
    let doc = drawing(3, 2);
    let (out, parts, _) = saved(&doc, &open_limits());
    out.unwrap();
    let encoded = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).unwrap();
    assert_eq!(parts[0].2, encoded.document);
    for (i, (id, tree)) in encoded.pages.iter().enumerate() {
        assert_eq!((parts[1 + i].0, &parts[1 + i].2), (PartRole::Page { id: *id }, tree));
    }
    for (i, (id, tree)) in encoded.masters.iter().enumerate() {
        assert_eq!((parts[1 + encoded.pages.len() + i].0, &parts[1 + encoded.pages.len() + i].2), (PartRole::Master { id: *id }, tree));
    }
}

#[test]
fn a_package_made_a_part_at_a_time_is_opened_by_an_open_that_asks_for_a_part_at_a_time() {
    let doc = drawing(4, 2);
    let (out, parts, blobs) = saved(&doc, &open_limits());
    out.unwrap();
    // Each part written and read again as a package would hold it.
    let limits = open_limits().xml;
    let (mut document, mut pages, mut masters) = (None, Vec::new(), Vec::new());
    for (role, _, tree) in &parts {
        let reread = parse_with(&to_bytes(tree, &[], &[], true).unwrap(), &limits).unwrap();
        match role {
            PartRole::Document => document = Some(reread),
            PartRole::Page { .. } => pages.push(reread),
            PartRole::Master { .. } => masters.push(reread),
        }
    }
    let mut source = Lazy::new(pages, masters);
    let back = decode_from(&document.unwrap(), &mut source, &blobs, &mut Budget::for_open(&open_limits()), &mut Vec::new()).unwrap();
    assert_eq!(back, doc);
    assert_eq!(source.most_live, 1);
}

#[test]
fn a_drawing_past_a_cap_of_a_part_is_refused_before_the_first_part_is_handed_over() {
    // A page 11 levels deep against a limit of 10 levels.
    let mut doc = drawing(2, 0);
    let mut deepest = Shape::new(1);
    for id in 2..11u32 {
        let mut outer = Shape::new(id);
        outer.children = ShapeList::from_vec(vec![Arc::new(deepest)]);
        deepest = outer;
    }
    let mut p = Page::new(9, "deep");
    p.shapes = ShapeList::from_vec(vec![Arc::new(deepest)]);
    doc.pages.push(Arc::new(p));
    let limits = Limits { xml: XmlLimits { max_depth: 10, ..open_limits().xml }, ..open_limits() };
    let (out, parts, _) = saved(&doc, &limits);
    assert_eq!(out, Err(CodecError::Limit { what: "levels of nested elements in one part", max: 10 }));
    assert!(parts.is_empty(), "nothing was handed over: {} parts", parts.len());
    // The package caps: the parts as written and the number of parts, each one short of what the drawing is.
    let doc = drawing(3, 0);
    let (out, parts, _) = saved(&doc, &open_limits());
    out.unwrap();
    let sizes: Vec<u64> = parts.iter().map(|(_, _, t)| diacraft_sheetxml::model::written_len(t)).collect();
    let total: u64 = sizes.iter().sum();
    let largest = sizes.iter().copied().max().unwrap();
    let (out, parts, _) = saved(&doc, &Limits { max_package_bytes: total - 1, ..open_limits() });
    assert_eq!(out, Err(CodecError::Limit { what: "bytes in the package as written", max: total - 1 }));
    assert!(parts.is_empty());
    assert!(saved(&doc, &Limits { max_package_bytes: total, ..open_limits() }).0.is_ok(), "the package as large as the parts is saved");
    let (out, parts, _) = saved(&doc, &Limits { max_parts: 3, ..open_limits() });
    assert_eq!(out, Err(CodecError::Limit { what: "parts in the package", max: 3 }));
    assert!(parts.is_empty());
    assert!(saved(&doc, &Limits { max_parts: 4, ..open_limits() }).0.is_ok());
    let (out, parts, _) = saved(&doc, &Limits { max_part_bytes: largest - 1, ..open_limits() });
    assert_eq!(out, Err(CodecError::Limit { what: "bytes in one part as written", max: largest - 1 }));
    assert!(parts.is_empty());
    assert!(saved(&doc, &Limits { max_part_bytes: largest, ..open_limits() }).0.is_ok());
    let (out, parts, _) = saved(&doc, &Limits { xml: XmlLimits { max_nodes: 10, ..open_limits().xml }, ..open_limits() });
    assert!(matches!(out, Err(CodecError::Limit { what: "XML items in one part", max: 10 })), "{out:?}");
    assert!(parts.is_empty());
}

#[test]
fn an_emit_that_fails_ends_the_save_with_its_error_and_no_part_after_it_is_made() {
    let doc = drawing(4, 1);
    let mut blobs = MemBlobs::new();
    let mut seen = 0;
    let out = save_parts(&doc, &mut blobs, &open_limits(), &mut |_| {
        seen += 1;
        if seen == 3 { Err(malformed("document", "the package could not take the part")) } else { Ok(()) }
    });
    assert_eq!(out, Err(malformed("document", "the package could not take the part")));
    assert_eq!(seen, 3);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// A save a part at a time is the save that holds every part: it hands over what that returns, or it is refused as that is.
    #[test]
    fn a_save_a_part_at_a_time_is_the_save_that_holds_every_part(doc in any_document(ModelSize::Small), tight in 0u8..4) {
        let limits = match tight {
            0 => open_limits(),
            1 => Limits { max_package_bytes: 60_000, ..open_limits() },
            2 => Limits { max_part_bytes: 6_000, ..open_limits() },
            _ => Limits { xml: XmlLimits { max_nodes: 400, ..open_limits().xml }, ..open_limits() },
        };
        let (out, parts, _) = saved(&doc, &limits);
        let held = encode_for_save(&doc, &mut MemBlobs::new(), &limits);
        match (out, held) {
            (Ok(()), Ok(encoded)) => {
                prop_assert_eq!(&parts[0].2, &encoded.document);
                prop_assert_eq!(parts.len(), 1 + encoded.pages.len() + encoded.masters.len());
                for (i, (_, tree)) in encoded.pages.iter().chain(encoded.masters.iter()).enumerate() {
                    prop_assert_eq!(&parts[1 + i].2, tree);
                }
            }
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            (a, b) => prop_assert!(false, "a part at a time says {a:?} and the save of every part says {:?}", b.map(|_| ())),
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- a check keeps nothing of a page

/// A source of pages that each show a picture of one blob under a part name of its own, and notes how many holders the blob's bytes have each time a page is
/// asked for: the decoder's cache of the blob, the pictures it cached (a few of each blob), and the pictures of the pages it kept.
struct Pictures {
    pages: Vec<El>,
    blob: Arc<[u8]>,
    holders: Vec<usize>,
}

impl PartSource for Pictures {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        self.holders.push(Arc::strong_count(&self.blob));
        self.pages.get(index).cloned().ok_or_else(|| malformed("document", "no such part"))
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(malformed("document", "no such part"))
    }
}

impl diacraft_sheetxml::model::BlobSource for Pictures {
    fn get(&self, _id: diacraft_sheetxml::model::BlobId) -> Option<Arc<[u8]>> {
        Some(Arc::clone(&self.blob))
    }
}

/// `n` pages that each show the blob under a part name of their own, and the document part that lists them and the blob.
fn picture_pages(n: usize) -> (El, Pictures) {
    let blob: Arc<[u8]> = Arc::from(vec![7u8; 100]);
    let stamp = diacraft_sheetxml::model::stamp_of_bytes(&blob).text();
    let d = "diacraft";
    let refs: String = (0..n).map(|k| format!("<{d}:page-ref id=\"{k}\" part=\"pages/page{k}.xml\"/>")).collect();
    let document = format!(
        "<{d}:document xmlns:{d}=\"{URN}\"><{d}:blob id=\"0\" path=\"blobs/0.bin\" media-type=\"image/png\" size=\"100\" stamp=\"{stamp}\"/>{refs}</{d}:document>"
    );
    let pages = (0..n)
        .map(|k| {
            let xml = format!(
                "<{d}:page xmlns:{d}=\"{URN}\" id=\"{k}\"><{d}:shape id=\"1\"><{d}:foreign><{d}:picture part=\"/media/p{k}.png\" type=\"image/png\" blob=\"0\"/></{d}:foreign></{d}:shape></{d}:page>"
            );
            parse_with(xml.as_bytes(), &open_limits().xml).unwrap()
        })
        .collect();
    (parse_with(document.as_bytes(), &open_limits().xml).unwrap(), Pictures { pages, blob, holders: Vec::new() })
}

#[test]
fn a_check_that_keeps_nothing_lets_each_page_go_and_a_decode_keeps_them() {
    let n = 30;
    // A check: the blob's bytes are held by the decoder's cache and by a few pictures it cached, whatever the number of pages that came before.
    let (document, mut source) = picture_pages(n);
    let blobs = Pictures { pages: Vec::new(), blob: Arc::clone(&source.blob), holders: Vec::new() };
    check_decodes_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(source.holders.len(), n);
    let most = source.holders.iter().copied().max().unwrap();
    assert!(most <= 12, "the pages of a check are let go as they are read: the bytes had {most} holders at most, {:?}", source.holders);
    // A decode keeps the pages, and each shows a picture of its own that holds the bytes.
    let (document, mut source) = picture_pages(n);
    let blobs = Pictures { pages: Vec::new(), blob: Arc::clone(&source.blob), holders: Vec::new() };
    let doc = decode_from(&document, &mut source, &blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(doc.pages.len(), n);
    let last = source.holders.last().copied().unwrap();
    assert!(last >= n, "a decode keeps its pages, and the picture of each: {last} holders at the last page, {:?}", source.holders);
}

#[test]
fn the_tree_of_a_document_part_is_held_back_from_the_model_before_anything_is_read_and_a_small_one_costs_nothing() {
    use diacraft_sheetxml::model::OPEN_DOCUMENT_FREE_ITEMS;
    let doc = drawing(2, 0);
    let (document, mut source, blobs) = lazy_of(&doc);
    // A document part of ordinary size: what a decode spends is what it spent before, whatever is held back for it.
    let mut spent = Budget::new(HUGE, HUGE, HUGE);
    decode_from(&document, &mut source, &blobs, &mut spent, &mut Vec::new()).unwrap();
    // A document part of 1.3 million items (650,000 elements of a kind nobody knows, each with a declaration): 1,044,000 of them are past what is free, and at 160 bytes each they
    // are 167 MB, which a pool of 100 MB does not have: refused in the words of the pool before anything of the part is read.
    let padding = "<x:y xmlns:x=\"urn:q\"/>".repeat(650_000);
    let text = to_bytes(&document, &[], &[], false).unwrap();
    let text = String::from_utf8(text).unwrap().replacen("</diacraft:document>", &format!("{padding}</diacraft:document>"), 1);
    let big = parse_with(text.as_bytes(), &XmlLimits { max_depth: 512, ..XmlLimits::default() }).unwrap();
    let items = 1 + big.attrs.len() as u64 + big.elements().map(|e| 1 + e.attrs.len() as u64 + e.children.len() as u64).sum::<u64>();
    assert!(items > OPEN_DOCUMENT_FREE_ITEMS + 1_000_000, "{items} items");
    let (_, mut again, blobs) = lazy_of(&doc);
    let mut small = Budget::new(HUGE, 100_000_000, HUGE);
    let out = decode_from(&big, &mut again, &blobs, &mut small, &mut Vec::new());
    assert_eq!(out.unwrap_err(), CodecError::Limit { what: "bytes of the model in memory", max: 100_000_000 });
    assert_eq!(small.bytes, 100_000_000, "nothing of it was read");
    // And with room for it, the tree is given back when the decode is done: what is left is all the model did not spend.
    let mut roomy = Budget::new(HUGE, HUGE, HUGE);
    let (_, mut third, blobs) = lazy_of(&doc);
    decode_from(&big, &mut third, &blobs, &mut roomy, &mut Vec::new()).unwrap();
    assert!(roomy.bytes < HUGE, "the model was charged");
    // Given back: the whole of what is left can be charged (a tree still held back would leave less than it asks for).
    let left = roomy.bytes;
    roomy.charge_bytes(left).expect("the tree was given back");
    assert!(spent.bytes <= HUGE);
}
