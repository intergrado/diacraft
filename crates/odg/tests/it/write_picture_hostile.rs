//! A new picture in a package, hostile inputs: ten thousand pictures, the same picture ten thousand times, a package that holds ten thousand pictures already, and
//! random sets of pictures (types, sizes, copies of the same bytes) saved, read again and saved again. The cost is counted, never timed: each count grows with the pictures
//! (and the entries the package holds) and not with their product.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_odg::{Limits, WriteWork, read};
use proptest::prelude::*;

use crate::drawing::Drawing;
use crate::flat_write::small_png;
use crate::write_core::{custom, open_bytes};
use crate::write_new::push_shape;
use crate::write_picture::{PNG_HEAD, entry_names, manifest_rows, named, noise, picture, pictures_of, save, shown, three};

/// The counts of a save that a picture's cost shows in: the looks in the sets of names, the bytes looked at and copied, the attributes written and the shapes compared.
fn counts(work: &WriteWork) -> [u64; 5] {
    [work.manifest_probes, work.scan_bytes, work.bytes_copied, work.attributes_written, work.shapes_compared]
}

/// Ten thousand pictures of their own, one frame each, in one save: ten thousand entries, ten thousand rows, ten thousand frames that each name their own entry, and each reads
/// back as its own picture. A second save through the new index is no edit.
#[test]
fn ten_thousand_new_pictures_are_ten_thousand_entries_rows_and_frames_and_read_back_each_its_own() {
    let mut o = three();
    let first = o.index.next_id(0).unwrap();
    let n = 10_000u32;
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for k in 0..n {
        let bytes = noise(&PNG_HEAD, 24, u64::from(k) + 1);
        assert!(seen.insert(bytes.clone()), "the pictures are told apart by their bytes");
        push_shape(&mut o.doc, picture(first + k, &format!("P{k}"), (1.0 + f64::from(k % 18), 8.0, 0.5, 0.5), &bytes));
    }
    let before = manifest_rows(&o.bytes).len();
    let saved = save(&o.doc, &o.bytes, &o.index).expect("ten thousand pictures are written");
    assert_eq!(pictures_of(&saved.bytes).len(), n as usize);
    assert_eq!(manifest_rows(&saved.bytes).len(), before + n as usize);
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("picture_missing"), 0);
    for k in [0u32, 1, 4_999, 9_998, 9_999] {
        let want = noise(&PNG_HEAD, 24, u64::from(k) + 1);
        assert_eq!(shown(named(&back, &format!("P{k}"))).map(|s| s.0), Some(want), "P{k}");
    }
    let shown_all: HashSet<Vec<u8>> = back.pages.get(0).unwrap().shapes.iter().filter_map(|s| shown(s).map(|p| p.0)).collect();
    assert_eq!(shown_all.len(), n as usize, "every picture is its own");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The cost grows with the pictures: four times the pictures costs about four times each count (a name's search from the first number for each picture, or a look through
/// the pictures made for each, would be sixteen times).
#[test]
fn the_cost_of_the_pictures_of_a_save_grows_with_their_number_and_not_with_its_square() {
    let cost = |n: u32| {
        let mut o = three();
        let first = o.index.next_id(0).unwrap();
        for k in 0..n {
            push_shape(&mut o.doc, picture(first + k, &format!("P{k}"), (1.0, 8.0, 0.5, 0.5), &noise(&PNG_HEAD, 64, u64::from(k) + 1)));
        }
        counts(&save(&o.doc, &o.bytes, &o.index).unwrap().work)
    };
    let (small, large) = (cost(500), cost(2_000));
    for (i, (a, b)) in small.iter().zip(&large).enumerate() {
        // Four times, and the digits of the names that a larger number needs (a count is a few bytes more for each picture, and the share of the file's own bytes in it moves with the
        // length of the namespace the file names): a fiftieth beside, where sixteen times is what a search from the first for each picture would cost.
        assert!(*b <= *a * 4 + *a / 50 + 64, "count {i}: {a} for 500 pictures and {b} for 2,000");
        assert!(*b * 3 >= *a * 4 || *a < 100, "count {i}: it grows, {a} and {b}");
    }
}

/// One picture that ten thousand frames show is one entry, and the work to tell so is one look at its bytes and not one for each frame: a picture of 5,000 bytes costs the
/// scan 4,950 bytes more than one of 50 whatever the number of frames that show it (the rest of the scan is the part's own and is the same).
#[test]
fn the_same_picture_ten_thousand_times_is_one_entry_and_one_look_at_its_bytes() {
    let run = |n: u32, size: usize| {
        let shared = picture(0, "x", (1.0, 8.0, 0.5, 0.5), &noise(&PNG_HEAD, size, 9));
        let mut o = three();
        let first = o.index.next_id(0).unwrap();
        for k in 0..n {
            let mut shape = shared.clone();
            shape.id = first + k;
            shape.name_u = Some(Arc::from(format!("P{k}")));
            shape.name = shape.name_u.clone();
            push_shape(&mut o.doc, shape);
        }
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
        assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"], "{n} frames");
        saved.work
    };
    for n in [1_000u32, 10_000] {
        let (big, small) = (run(n, 5_000), run(n, 50));
        assert_eq!(big.scan_bytes - small.scan_bytes, 4_950, "{n} frames: the bytes were looked at once");
    }
    let (a, b) = (run(1_000, 50), run(10_000, 50));
    assert!(b.manifest_probes <= a.manifest_probes + 8, "one name, tried once: {} and {}", a.manifest_probes, b.manifest_probes);
}

/// A package that holds many pictures of the names the writer gives already: the cursor passes each once, so the cost is the entries the package holds and the pictures made,
/// and four times the entries costs about four times.
#[test]
fn a_package_that_holds_ten_thousand_pictures_costs_the_entries_it_holds_once_over() {
    let png = small_png();
    let cost = |held: u32| {
        let mut d: Drawing = Drawing::a4().page(&custom("s1", "1cm"));
        for k in 1..=held {
            d.files.push((format!("Pictures/image{k}.png"), png.clone()));
        }
        let mut o = open_bytes(d.bytes());
        let first = o.index.next_id(0).unwrap();
        for k in 0..100u32 {
            push_shape(&mut o.doc, picture(first + k, &format!("P{k}"), (1.0, 8.0, 0.5, 0.5), &noise(&PNG_HEAD, 64, u64::from(k) + 1)));
        }
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
        let names = pictures_of(&saved.bytes);
        assert_eq!(names.len(), held as usize + 100);
        assert!(names.contains(&format!("Pictures/image{}.png", held + 100)), "the names run on from the last one held");
        (saved.work.manifest_probes, entry_names(&saved.bytes).len())
    };
    let ((small, _), (large, entries)) = (cost(2_500), cost(10_000));
    assert!(large <= small * 4 + 64, "{small} for 2,500 held and {large} for 10,000");
    assert!(entries > 10_000);
}

/// A picture of a kind and size chosen by a seed: a signature of one of the five types, or none that the reader draws, and a tail of bytes.
fn seeded(kind: u8, len: u8, seed: u8) -> Vec<u8> {
    let head: Vec<u8> = match kind % 8 {
        0 => PNG_HEAD.to_vec(),
        1 => vec![0xFF, 0xD8, 0xFF],
        2 => b"GIF89a".to_vec(),
        3 => b"BM".to_vec(),
        4 => {
            let mut v = vec![0u8; 40];
            v.extend_from_slice(b" EMF");
            v
        }
        5 => vec![0xD7, 0xCD, 0xC6, 0x9A],
        6 => b"<svg".to_vec(),
        _ => Vec::new(),
    };
    let mut bytes = head;
    bytes.extend((0..usize::from(len)).map(|i| (i as u8).wrapping_mul(seed | 1).wrapping_add(seed)));
    bytes
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, failure_persistence: None, ..ProptestConfig::default() })]

    /// Random sets of pictures: when every picture is one the reader draws, the save writes one entry for each distinct bytes, each frame reads back as its picture, a second
    /// save through the new index is no edit, and the file opened again and edited elsewhere keeps every entry; when one is not, the whole save is refused in words.
    #[test]
    fn random_sets_of_pictures_are_written_read_back_and_saved_again(shapes in proptest::collection::vec((any::<u8>(), 0u8..48, any::<u8>(), any::<bool>()), 1..12)) {
        let mut o = three();
        let first = o.index.next_id(0).unwrap();
        let mut bytes: Vec<Vec<u8>> = Vec::new();
        for (k, (kind, len, seed, copy)) in shapes.iter().enumerate() {
            // A copy shows the bytes of the picture before it (the same bytes, in an `Arc` of its own).
            let b = match (copy, bytes.last()) { (true, Some(previous)) => previous.clone(), _ => seeded(*kind, *len, *seed) };
            push_shape(&mut o.doc, picture(first + k as u32, &format!("P{k}"), (1.0 + k as f64, 8.0, 0.5, 0.5), &b));
            bytes.push(b);
        }
        // The oracle is the signatures, written here and not taken from the writer.
        let drawn = |b: &Vec<u8>| {
            matches!(b.as_slice(), [0x89, b'P', b'N', b'G', ..] | [0xFF, 0xD8, ..] | [b'G', b'I', b'F', b'8', ..] | [b'B', b'M', ..]) || b.get(40..44) == Some(&b" EMF"[..])
        };
        let result = save(&o.doc, &o.bytes, &o.index);
        if !bytes.iter().all(drawn) {
            let e = result.expect_err("a picture the reader does not draw refuses the save");
            prop_assert_eq!(e.class(), "write_unsupported");
            return Ok(());
        }
        let saved = match result {
            Ok(saved) => saved,
            // The bytes of a seeded picture can deflate past the ratio the container reads: said in words.
            Err(e) => { prop_assert!(e.to_string().contains("ratio"), "{e}"); return Ok(()); }
        };
        let distinct: HashSet<&Vec<u8>> = bytes.iter().collect();
        prop_assert_eq!(pictures_of(&saved.bytes).len(), distinct.len());
        let (back, _): (Document, _) = read(&saved.bytes, &Limits::default()).unwrap();
        for (k, b) in bytes.iter().enumerate() {
            prop_assert_eq!(shown(named(&back, &format!("P{k}"))).map(|s| s.0), Some(b.clone()), "P{}", k);
        }
        let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
        prop_assert!(!again.edited && again.bytes == saved.bytes);
        // The file opened again, a shape moved: every entry is still there and read as before.
        let mut o2 = open_bytes(saved.bytes.clone());
        let s1 = named(&o2.doc, "s1").id;
        crate::write_core::move_by(&mut o2.doc, 0, s1, 1.0, 0.0);
        let third = save(&o2.doc, &o2.bytes, &o2.index).unwrap();
        prop_assert_eq!(pictures_of(&third.bytes), pictures_of(&saved.bytes));
    }
}
