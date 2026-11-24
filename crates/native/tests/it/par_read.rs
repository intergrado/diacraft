//! **A `.diacraft` file read on threads is the file read on one** (`Limits::threads`): the blobs the document names (its pictures) are inflated on threads, each into the buffer the pool was
//! charged for, and the document, the notes, what the read cost and every refusal are the single-threaded read's at 2, 7 and 31 threads.

use std::sync::Arc;

use diacraft_native::{Limits, NativeError, Opened, read};

use crate::util::*;

const THREADS: [usize; 4] = [1, 2, 7, 31];

/// Bytes of a picture that is deflated in the package (it starts with no signature a blob is stored for) and takes the compressor's time: four bits in each byte.
fn deflated_picture(len: usize, mut seed: u64) -> Arc<[u8]> {
    let mut out = b"BM".to_vec();
    while out.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        out.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    out.truncate(len.max(2));
    Arc::from(out)
}

/// Pictures of 12 sizes (each its own bytes, the first eight deflated and the rest stored), four pages of three.
fn drawing() -> diacraft_doc::Document {
    let sizes = [300_000usize, 90_000, 2_000, 400_000, 70_000, 100, 150_000, 80_000, 3_000, 260_000, 66_000, 500];
    let pictures: Vec<Arc<[u8]>> =
        sizes.iter().enumerate().map(|(k, len)| if k < 8 { deflated_picture(*len, k as u64 + 1) } else { png(k as u8 + 1, *len) }).collect();
    let pages = (0..4).map(|p| page_showing(p as u32 + 1, &[&pictures[p * 3], &pictures[p * 3 + 1], &pictures[p * 3 + 2]])).collect();
    drawing_of(pages)
}

type Outcome = Result<(diacraft_doc::Document, Opened), NativeError>;

fn read_on(bytes: &[u8], limits: &Limits, threads: usize) -> Outcome {
    read(bytes, &Limits { threads, ..limits.clone() })
}

/// The reads at every count of threads are the read at one: the document, the notes, what the read cost, the cost of the blobs it kept, and the error.
fn assert_alike(bytes: &[u8], limits: &Limits, what: &str) -> Outcome {
    let one = read_on(bytes, limits, 1);
    for threads in THREADS.iter().copied().filter(|t| *t > 1) {
        let many = read_on(bytes, limits, threads);
        match (&one, &many) {
            (Ok((doc, opened)), Ok((doc2, opened2))) => {
                assert!(doc == doc2, "{what}: the document at {threads} threads: {}", first_difference(doc2, doc).unwrap_or_default());
                assert_eq!(opened.notes, opened2.notes, "{what}: the notes at {threads} threads");
                assert_eq!(opened.work, opened2.work, "{what}: what the read cost at {threads} threads");
                assert_eq!(opened.decoded.blobs(), opened2.decoded.blobs(), "{what}: the blobs kept at {threads} threads");
                assert_eq!(opened.kept_parts, opened2.kept_parts);
            }
            (Err(a), Err(b)) => assert_eq!((a.to_string(), a.class()), (b.to_string(), b.class()), "{what}: the refusal at {threads} threads"),
            _ => panic!("{what}: read at one thread: {}, at {threads}: {}", one.is_ok(), many.is_ok()),
        }
    }
    one
}

#[test]
fn a_drawing_with_many_pictures_is_the_same_document_and_cost_at_any_number_of_threads() {
    let bytes = write_doc(&drawing());
    let (doc, opened) = assert_alike(&bytes, &limits(), "12 pictures").expect("the drawing reads");
    assert_eq!(opened.decoded.blobs(), 12);
    // At one thread each blob is inflated when the decode asks for it; at more, the blobs worth a thread (the deflated ones of 64 KiB and over: six of the twelve; a stored one is copied at the
    // speed of memory) are inflated ahead of it.
    assert_eq!(opened.blobs_ahead, 0);
    let (_, ahead) = read_on(&bytes, &limits(), 7).unwrap();
    assert_eq!(ahead.blobs_ahead, 6, "the blobs that threads inflated before the decode asked for them");
    assert_eq!(opened.notes, []);
    assert_eq!(doc.pages.len(), 4);
}

#[test]
fn a_blob_that_is_damaged_is_lost_and_noted_alike_at_any_number_of_threads() {
    // A deflated picture whose compressed bytes are changed (it does not inflate to what it says), and a stored one whose byte is changed (its checksum fails): each is lost with a note and the
    // pictures after it are read, whichever thread inflated which.
    let doc = drawing();
    let bytes = write_doc(&doc);
    let archive = diacraft_container::zip::Archive::read(&bytes, &diacraft_container::Limits::default()).unwrap();
    let blob = |bytes_of: &Arc<[u8]>| -> (usize, usize) {
        let entry = archive.entries().iter().find(|e| e.size == bytes_of.len() as u64 && e.name.starts_with("blobs/")).expect("the blob's entry");
        (entry.data_offset, usize::try_from(entry.compressed).unwrap())
    };
    for (what, shape, flip) in [("a deflated picture", 0usize, true), ("a stored picture", 6, false)] {
        let mut damaged = bytes.clone();
        let picture = picture_of(&doc, shape / 3, shape % 3).unwrap();
        let (start, length) = blob(&picture);
        damaged[start + length / 2] ^= if flip { 0xFF } else { 0x55 };
        let (back, opened) = assert_alike(&damaged, &limits(), what).expect("a damaged blob does not refuse the file");
        assert_eq!(opened.notes.len(), 1, "{what}: {:?}", opened.notes);
        assert!(picture_of(&back, shape / 3, shape % 3).is_none(), "{what} is lost");
        assert!(picture_of(&back, shape / 3, shape % 3 + 1).is_some() || shape % 3 == 2, "{what}: the picture after it is read");
    }
}

#[test]
fn a_refusal_for_any_memory_is_the_same_words_at_any_number_of_threads() {
    // The memory of the read is walked from too little to enough: the pools shrink with it, and a file that is refused at one thread is refused at 31 with the same words, in the same place
    // (each blob is charged to the pool before any is inflated), and one that is read at one is read at 31.
    let bytes = write_doc(&drawing());
    let (mut refused, mut read) = (0, 0);
    // Where the file starts to be read, found by halving between a memory that refuses it and the one that reads it.
    let (mut low, mut high) = (1u64, limits().max_memory);
    assert!(
        read_on(&bytes, &Limits { max_memory: low, ..limits() }, 1).is_err() && read_on(&bytes, &Limits { max_memory: high, ..limits() }, 1).is_ok()
    );
    while high - low > 100_000 {
        let middle = low + (high - low) / 2;
        if read_on(&bytes, &Limits { max_memory: middle, ..limits() }, 1).is_ok() { high = middle } else { low = middle }
    }
    for step in 0..=40u64 {
        let max_memory = high / 2 + high * 3 / 2 * step / 40;
        let limits = Limits { max_memory, ..limits() };
        if assert_alike(&bytes, &limits, &format!("max_memory {max_memory}")).is_ok() {
            read += 1;
        } else {
            refused += 1;
        }
    }
    assert!(refused > 3 && read > 3, "the walk crosses the edge: {refused} refused, {read} read");
}

#[test]
fn a_package_whose_blob_record_says_more_than_the_entry_holds_is_alike_too() {
    // A blob whose entry is a different size than its record says is not this blob (nothing is inflated for it): lost, at any count.
    let doc = drawing();
    let bytes = write_doc(&doc);
    let document = String::from_utf8(entry(&bytes, "document.xml")).unwrap();
    let at = document.find("size=\"400000\"").expect("the record of the largest picture");
    let wrong = format!("{}size=\"400001\"{}", &document[..at], &document[at + "size=\"400000\"".len()..]);
    let mut items = entries(&bytes);
    for (name, data) in &mut items {
        if name == "document.xml" {
            *data = wrong.clone().into_bytes();
        }
    }
    let items: Vec<crate::zipbuild::Item> = items.iter().map(|(name, data)| crate::zipbuild::Item::stored(name, data)).collect();
    let _ = assert_alike(&package(&items), &limits(), "a record that says another size");
}
