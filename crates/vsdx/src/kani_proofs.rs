//! Kani proof harnesses for the integer code of the VSDX writer: the place of a typed child among the unknown nodes the model writer weaves back
//! (`write::model::typed_place`) and the cap of a part of DiaCraft's (`carry::write::too_large`); and the byte code of the objects a save writes
//! (`write::objects::is_compound`, `sextet`, `too_large`, and `write::model::picture_by_bytes`); and, the numbers of the
//! comments part (`comments::number`). Run with `cargo xtask kani`
//! (`docs/development.md`, Proofs).
//!
//! **Not proved by **, each with its reason: `object_number` and `greatest_number` (the last segment found by `rsplit`, the memchr trap, of names of fourteen characters and
//! more); `inline_bytes` (it reads a `RawElement`); `picture_content_type` (a match of content types); `without_object`, `Embeddings::natively` (`Ext` slots and hash sets);
//! the `Ids::fresh`, `row_of` and `gone_row` (maps keyed by strings, and a cache); the `may_be_id` and `local` (`split_once` and `rsplit`), `Frame::bound`
//! (a chain of frames of strings), `Found::look` (a hash set), `style_keys` (maps) and `shapes_with_data` (a shape list); the `ahead::reservation` (the arithmetic of a slice's
//! length: only short slices can be made, far from where it saturates) and the counters of `Ahead` (a state behind a lock that threads share).
//!
//! **Not proved:** `carry::odf_entry_name_ok` of every ASCII-case spelling of `meta.xml`, `settings.xml` and `meta.xml/` (the stand-ins): the three harnesses
//! ran CBMC out of 20 GB (two) and past 300 s (the third), three at a time, through the name's `split('/')`, `contains`, `chars` and `to_ascii_lowercase` (the string trap), though only the case of
//! each letter was symbolic; `holds::rels_of`'s `left` (one saturating subtraction inside the read of a package's relationships). The unit tests hold them.
//!
//! **Not proved here**, each with its reason: `carry::content_type_of` (CARRYVSDX; a media type of two to five characters by its kinds of character took 176 to 192 s a
//! length, past the 150 s a harness is kept under: the search of `split_once` for the slash is a scan of the text a word at a time); `read_back_failed` (every
//! failure but the two of memory is told in the error's words, `to_string`, a formatting that Kani does not finish; its unit tests hold the three cases);
//! `carry::read::copy_cost` and `Reader::spend` (CARRYVSDX; they read the table of stamps and the state of a read, which hold hash maps, the trap of
//! `docs/development.md`); the names of the carrier (`odf_entry_name_ok`, `native_entry_name_ok`, `odf_macro_name`, `odf_role_of`, `is_blob_target`: texts of up to
//! 1,024 bytes matched against a folder of ten, six reserved names and the rules of a part name, which a symbolic text of a few characters cannot reach; the unit tests
//! of `carry.rs` and of the carrier's integration tests enumerate them).

use diacraft_doc::{Passthrough, Placed, RawNode};

use crate::carry::too_large;
use crate::write::typed_place;

/// `too_large`: a part of DiaCraft's of any length is past the package's cap exactly when it is longer than `MAX_PART_BYTES`; **one byte over is over**.
#[kani::proof]
fn a_carried_part_is_too_large_exactly_when_it_is_past_the_packages_cap() {
    let len: usize = kani::any();
    let cap = diacraft_container::limits::MAX_PART_BYTES;
    assert!(too_large(len) == (len as u64 > cap));
    assert!(!too_large(cap as usize) && too_large(cap as usize + 1));
}

/// `typed_place`, for three unknown nodes that followed **any** numbers of typed children and **any** typed child: its place is its own number and one more for
/// each unknown node that followed no more typed children than it, which is where `diacraft_sheetxml::build::weave` puts it (its documentation; the tests hold the
/// two to each other on real pages); so the places of two children are in their order and never one (each child is past the one before it, and no further than the unknown
/// nodes that stand between), and nothing wraps, at the largest number too.
///
/// **Not proved: the place against `weave` itself.** A harness that weaves three typed children and one unknown node and looks the child up at its place ran past 180 s, and
/// one of two unknown nodes past 300 s: what is left of the nodes `weave` did not take is dropped, a count the solver does not know, and the drop of a node, which holds
/// nodes, is unrolled at every level to the bound.
#[kani::proof]
#[kani::unwind(5)]
fn the_places_of_the_typed_children_are_in_their_order_and_never_shared() {
    let afters: [u32; 3] = kani::any();
    let passthrough =
        Passthrough::from_nodes(afters.iter().map(|after| Placed { after: *after, node: RawNode::Text(std::sync::Arc::from("")) }).collect());
    let at: usize = kani::any();
    let place = typed_place(&passthrough, at);
    let before = afters.iter().filter(|after| (**after as usize) <= at).count();
    assert!(place == at.saturating_add(before));
    if let Some(next) = at.checked_add(1) {
        let then = typed_place(&passthrough, next);
        assert!(then > place || place == usize::MAX);
        assert!(then <= place.saturating_add(1 + 3));
    }
    std::mem::forget(passthrough);
    kani::cover!(at.checked_add(3) == Some(place), "every unknown node before the child");
    kani::cover!(at == usize::MAX, "the largest place");
}

// ---: the objects a VSDX save writes ---

/// The bytes a compound file begins with.
const COMPOUND_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The longest bytes the harness of `is_compound` draws: room for the root entry of the first directory sector of a version 3 file (512 bytes a sector, the entry 128 bytes at
/// sector 0, which starts at 512), and 64 bytes more.
const COMPOUND_LEN: usize = 704;

/// `write::objects::is_compound` (the five steps) for **every** bytes of up to [`COMPOUND_LEN`]: a compound file exactly when there are at least 512 bytes that
/// begin with the signature, the byte order mark at 0x1C is `0xFFFE`, the version and the sector shift at 0x1A and 0x1E are (3, 9) or (4, 12), the first directory sector at 0x30
/// is below `0xFFFF_FFFA`, the entry it starts, at the sector's number and one times the sector's size, lies whole inside the bytes, and that entry's type at 0x42 is 5 (a root). It
/// reads nothing past the bytes and never panics, whatever they hold. (A version 4 file's first directory sector starts at 4,096: within these lengths it is refused for lying outside,
/// which is all its true case adds.)
#[kani::proof]
#[kani::unwind(10)]
fn bytes_are_a_compound_file_exactly_by_the_five_steps_of() {
    use crate::write::is_compound;
    let buf: [u8; COMPOUND_LEN] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= COMPOUND_LEN);
    let bytes = buf.get(..len).unwrap_or(&[]);
    let got = is_compound(bytes);
    let u16_at = |at: usize| u16::from(buf[at]) | (u16::from(buf[at + 1]) << 8);
    let first = u32::from(buf[0x30]) | (u32::from(buf[0x31]) << 8) | (u32::from(buf[0x32]) << 16) | (u32::from(buf[0x33]) << 24);
    let (version, order, shift) = (u16_at(0x1A), u16_at(0x1C), u16_at(0x1E));
    let header =
        len >= 512 && buf[..8] == COMPOUND_SIGNATURE && order == 0xFFFE && matches!((version, shift), (3, 9) | (4, 12)) && first < 0xFFFF_FFFA;
    let want = header && {
        let at = (u128::from(first) + 1) << shift;
        at + 128 <= len as u128 && buf[at as usize + 0x42] == 5
    };
    assert!(got == want);
    kani::cover!(got, "a compound file");
    kani::cover!(header & !got, "a header whose root entry is not there or not a root");
}

/// `write::model::picture_by_bytes` for **every** bytes of up to 44: a picture exactly for the first bytes of the six formats a drawing draws (PNG's `89 'PNG'`, JPEG's
/// `FF D8 FF`, GIF's `'GIF8'`, BMP's `'BM'`, TIFF's `'II' 2A 00` or `'MM' 00 2A`, and EMF's record 1 with `' EMF'` at 40), with the extension and the content type of that format; and
/// no picture for any other bytes, the empty ones and an EMF's first record that is cut short included.
#[kani::proof]
#[kani::unwind(13)] // the content types compared, up to 11 bytes
fn the_first_bytes_of_a_picture_say_its_format_exactly_for_the_six_a_drawing_draws() {
    use crate::write::picture_by_bytes;
    let buf: [u8; 44] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 44);
    let bytes = buf.get(..len).unwrap_or(&[]);
    let starts = |sig: &[u8]| len >= sig.len() && buf[..sig.len()] == *sig;
    let want = if starts(&[0x89, b'P', b'N', b'G']) {
        Some((".png", "image/png"))
    } else if starts(&[0xFF, 0xD8, 0xFF]) {
        Some((".jpeg", "image/jpeg"))
    } else if starts(b"GIF8") {
        Some((".gif", "image/gif"))
    } else if starts(b"BM") {
        Some((".bmp", "image/bmp"))
    } else if starts(&[b'I', b'I', 0x2A, 0x00]) || starts(&[b'M', b'M', 0x00, 0x2A]) {
        Some((".tiff", "image/tiff"))
    } else if starts(&[1, 0, 0, 0]) && len >= 44 && buf[40..44] == *b" EMF" {
        Some((".emf", "image/x-emf"))
    } else {
        None
    };
    let got = picture_by_bytes(bytes);
    assert!(got == want);
    kani::cover!(got == Some((".emf", "image/x-emf")), "an EMF");
    kani::cover!(got.is_none() & starts(&[1, 0, 0, 0]), "an EMF's first record cut short, or of another signature");
}

/// `write::objects::sextet` (the characters of an inline object's base64) for **every** byte: the 64 characters of the standard alphabet are the values 0 to 63 in its
/// order (`A` to `Z`, `a` to `z`, `0` to `9`, `+`, `/`), each once; the padding `=` and an ASCII blank are skipped; and every other byte is no base64, which refuses the object.
#[kani::proof]
fn a_base64_character_is_its_place_in_the_alphabet_and_any_other_byte_is_refused() {
    use crate::write::sextet;
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b: u8 = kani::any();
    let place: usize = kani::any();
    kani::assume(place < 64);
    assert!(sextet(ALPHABET[place]) == Some(Some(place as u8)));
    let got = sextet(b);
    let in_alphabet = b.is_ascii_alphanumeric() || b == b'+' || b == b'/';
    match got {
        Some(Some(v)) => {
            assert!(in_alphabet && v < 64 && ALPHABET[usize::from(v)] == b);
        }
        None => {
            assert!(b == b'=' || b.is_ascii_whitespace());
        }
        Some(None) => {
            assert!(!in_alphabet && b != b'=' && !b.is_ascii_whitespace());
        }
    }
    kani::cover!(got == Some(None), "a byte that is no base64");
}

/// `write::objects::too_large` (the cap of an object's part): exactly a length past 256 MiB; one byte over is over.
#[kani::proof]
fn an_object_is_too_large_exactly_when_it_is_past_the_cap_of_a_part() {
    use crate::write::object_too_large;
    let len: usize = kani::any();
    let cap = diacraft_container::limits::MAX_PART_BYTES;
    assert!(object_too_large(len) == (len as u64 > cap));
    assert!(!object_too_large(cap as usize) && object_too_large(cap as usize + 1));
}

/// A byte of a comment part's number by its kind: a digit, a plus, a minus, a space or a letter.
fn number_byte(kind: u8, digit: u8) -> u8 {
    match kind % 5 {
        0 => b'0' + digit % 10,
        1 => b'+',
        2 => b'-',
        3 => b' ',
        _ => b'x',
    }
}

/// The law of `comments::number` for a text of `N` bytes: a number exactly when every byte is a digit and the digits' value fits a `u32`, and then that value.
fn comment_number_law<const N: usize>(bytes: [u8; N]) -> Option<u32> {
    let text = std::str::from_utf8(&bytes).unwrap_or("");
    let digits = bytes.iter().all(u8::is_ascii_digit);
    let mut value: u64 = 0;
    for b in bytes {
        value = value.saturating_mul(10).saturating_add(u64::from(b.wrapping_sub(b'0')));
    }
    let got = crate::comments::number(text);
    if digits && value <= u64::from(u32::MAX) {
        assert!(got.map(u64::from) == Some(value));
    } else {
        assert!(got.is_none());
    }
    got
}

/// `comments::number` (an `ID`, `PageID`, `ShapeID` or `AuthorID` of the comments part, digits and nothing else) for **every** text of one and of three bytes by
/// their kinds: a number exactly for digits alone, a sign or a blank refused; and none for no text.
#[kani::proof]
#[kani::unwind(12)]
fn a_comments_number_is_digits_and_nothing_else() {
    assert!(crate::comments::number("").is_none());
    let (k1, d1): ([u8; 1], [u8; 1]) = (kani::any(), kani::any());
    let one = comment_number_law([number_byte(k1[0], d1[0])]);
    let (k, d): ([u8; 3], [u8; 3]) = (kani::any(), kani::any());
    let three = comment_number_law([number_byte(k[0], d[0]), number_byte(k[1], d[1]), number_byte(k[2], d[2])]);
    kani::cover!(one == Some(9), "a digit");
    kani::cover!(three.is_some_and(|n| n < 100), "three digits from a 0");
    kani::cover!(three.is_none() & (k[0] % 5 == 1), "a plus before digits");
}

/// The same for **every** text of ten digits: the number exactly when it fits a `u32` (4,294,967,295 does, one more does not). (Eleven digits as well took 148 s at load 40;
/// they fit only from a 0, which the unit tests hold.)
#[kani::proof]
#[kani::unwind(12)]
fn a_comments_number_of_ten_digits_is_its_value_exactly_when_it_fits() {
    let ten: [u8; 10] = kani::any();
    kani::assume(ten.iter().all(u8::is_ascii_digit));
    let got = comment_number_law(ten);
    kani::cover!(got == Some(u32::MAX), "the largest");
    kani::cover!(got.is_none(), "ten digits past it");
}
