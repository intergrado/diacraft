//! Recognising a file by the fixed places of its first entry (the public spec's 2.3).
use diacraft_doc::Document;
use diacraft_native::{Kind, NativeError, State, WriteOptions, sniff, write};

use crate::util::*;
use crate::zipbuild::Item;

fn written(kind: Kind) -> Vec<u8> {
    let options = WriteOptions { kind, ..WriteOptions::default() };
    write(&Document::new_blank(), &State::default(), None, &options).unwrap().bytes
}

#[test]
fn the_mimetype_is_read_at_its_fixed_offset() {
    for kind in Kind::ALL {
        let bytes = written(kind);
        // `PK` at 0, `mimetype` at 30, the media type from 38: what a file manager's magic rule matches.
        assert_eq!(&bytes[..2], b"PK");
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..38 + kind.media_type().len()], kind.media_type().as_bytes());
        assert_eq!(sniff(&bytes), Some(kind), "{kind:?}");
    }
    // The kinds, their words, extensions and media types are the spec's table (1.2).
    assert_eq!(Kind::Drawing.media_type(), "application/vnd.diacraft.drawing");
    assert_eq!(Kind::Stencil.media_type(), "application/vnd.diacraft.stencil");
    assert_eq!(Kind::Template.media_type(), "application/vnd.diacraft.template");
    assert_eq!(
        (Kind::Drawing.extension(), Kind::Stencil.extension(), Kind::Template.extension()),
        (".diacraft", ".diacraft-stencil", ".diacraft-template")
    );
    assert_eq!((Kind::Drawing.word(), Kind::Stencil.word(), Kind::Template.word()), ("drawing", "stencil", "template"));
    for kind in Kind::ALL {
        assert_eq!(Kind::from_media_type(kind.media_type()), Some(kind));
        assert_eq!(Kind::from_word(kind.word()), Some(kind));
    }
    assert_eq!(Kind::from_media_type("application/vnd.diacraft.other"), None);
    assert_eq!(Kind::from_word("Drawing"), None, "words are compared byte for byte");
    // The extension is a hint only: the bytes decide, whatever the file is called. `sniff` does not even see a name.
    assert_eq!(sniff(&written(Kind::Template)), Some(Kind::Template));
}

#[test]
fn a_compressed_or_late_mimetype_is_not_sniffed() {
    let sound = package(&vector_1());
    assert_eq!(sniff(&sound), Some(Kind::Drawing));
    // Compressed.
    let mut items = vector_1();
    items[0] = Item::deflated("mimetype", MEDIA_DRAWING.as_bytes());
    let compressed = package(&items);
    assert_eq!(sniff(&compressed), None);
    assert_eq!(refused(&compressed), NativeError::NotNative);
    // Not the first entry.
    let mut items = vector_1();
    items.swap(0, 2);
    let late = package(&items);
    assert_eq!(sniff(&late), None);
    assert_eq!(refused(&late), NativeError::NotNative);
    // An extra field in the local header.
    let mut items = vector_1();
    items[0] = Item::stored("mimetype", MEDIA_DRAWING.as_bytes()).with_local_extra(&[0xCA, 0xFE, 0, 0]);
    let with_extra = package(&items);
    assert_eq!(sniff(&with_extra), None);
    assert_eq!(refused(&with_extra), NativeError::NotNative);
    // Another type, a type of a kind this build does not know, and bytes that are no archive at all.
    let mut items = vector_1();
    items[0] = Item::stored("mimetype", b"application/vnd.oasis.opendocument.graphics");
    assert_eq!(refused(&package(&items)), NativeError::NotNative);
    let mut items = vector_1();
    items[0] = Item::stored("mimetype", b"application/vnd.diacraft.scribble");
    let unknown = package(&items);
    assert_eq!(sniff(&unknown), None, "no kind to name");
    assert_eq!(refused(&unknown).class(), "kind_unknown", "but it starts as a diacraft file does, and is refused as one");
    for bytes in [&b""[..], b"PK", b"PK\x03\x04", b"not a package at all, but long enough to hold a local header and more", &[0u8; 100]] {
        assert_eq!(sniff(bytes), None);
        assert_eq!(refused(bytes), NativeError::NotNative);
    }
    // The name is at 30 and is `mimetype` exactly.
    let mut items = vector_1();
    items[0] = Item::stored("mimetypf", MEDIA_DRAWING.as_bytes());
    assert_eq!(sniff(&package(&items)), None);
    // A truncated file ends the read of the fixed places and is no file of the format.
    for cut in [0, 10, 30, 37, 40, 60] {
        assert_eq!(sniff(&sound[..cut.min(sound.len())]), if cut >= 38 + MEDIA_DRAWING.len() { Some(Kind::Drawing) } else { None }, "cut at {cut}");
    }
}

#[test]
fn the_opc_reader_refuses_a_diacraft_file() {
    // A `.diacraft` file is no Open Packaging Conventions package: it has no `[Content_Types].xml` (2.3), so the VSDX reader's package layer does not claim it.
    for bytes in [package(&vector_5()), written(Kind::Drawing)] {
        let error = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap_err();
        assert_eq!(error.class(), "missing_content_types");
    }
}

#[test]
fn a_kind_that_disagrees_with_the_manifest_is_damage() {
    // Vector 1 with `kind="stencil"` in the manifest and `mimetype` unchanged.
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest("1.0", "stencil", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
    );
    let error = refused(&package(&items));
    assert_eq!(error.class(), "kind_mismatch");
    assert!(error.to_string().contains("damaged"), "in words: {error}");
    // And the other way: a stencil's media type with a drawing's manifest.
    let mut items = vector_1();
    items[0] = Item::stored("mimetype", Kind::Stencil.media_type().as_bytes());
    assert_eq!(refused(&package(&items)).class(), "kind_mismatch");
    // A kind of the manifest that is no kind at all.
    let mut items = vector_1();
    replace(&mut items, "manifest.xml", manifest("1.0", "scribble", &[("document.xml", "document", "")]));
    assert_eq!(refused(&package(&items)).class(), "manifest_kind");
    // The kinds agree: a stencil file opens as a stencil, and its kind is the file's.
    let mut items = vector_1();
    items[0] = Item::stored("mimetype", Kind::Stencil.media_type().as_bytes());
    replace(
        &mut items,
        "manifest.xml",
        manifest("1.0", "stencil", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
    );
    assert_eq!(open_ok(&package(&items)).1.kind, Kind::Stencil);
}

/// A stored entry whose sizes and checksum are in a data descriptor after its data (flag bit 3), as a streaming writer makes one: the local header says 0 for the checksum and
/// both sizes, and the central row has the real ones. With `signature` the descriptor starts with `PK\x07\x08`.
fn with_descriptor(mut item: Item, signature: bool) -> Item {
    let mut descriptor = Vec::new();
    if signature {
        descriptor.extend_from_slice(&0x0807_4B50u32.to_le_bytes());
    }
    descriptor.extend_from_slice(&item.crc.to_le_bytes());
    descriptor.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
    descriptor.extend_from_slice(&(item.size as u32).to_le_bytes());
    item.flags |= 0x0008;
    item.local_sizes = Some((0, 0, 0));
    item.after_data = descriptor;
    item
}

#[test]
fn a_mimetype_with_a_data_descriptor_is_a_diacraft_file() {
    // 2.3 asks that the first entry be named `mimetype`, be stored, have no extra field and start with the prefix. It says nothing against a data descriptor (2.1's "no data
    // descriptor" is what a writer writes, and a reader refuses only the MUST rules): the sniff took the sizes of the local header, which are 0, and found no media type.
    for signature in [true, false] {
        for kind in Kind::ALL {
            let mut items = vector_1();
            items[0] = with_descriptor(Item::stored("mimetype", kind.media_type().as_bytes()), signature);
            let bytes = package(&items);
            assert_eq!(&bytes[30..38], b"mimetype");
            assert_eq!(sniff(&bytes), Some(kind), "{kind:?}, signature {signature}");
        }
    }
    // And the file opens: the container reads the sizes of the directory, and the manifest says the kind the `mimetype` does.
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()), true);
    let (_, opened) = open_ok(&package(&items));
    assert_eq!(opened.kind, Kind::Drawing);
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()), false);
    assert_eq!(open_ok(&package(&items)).1.kind, Kind::Drawing);
    // A stencil's media type with a drawing's manifest is the same damage as without a descriptor.
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", Kind::Stencil.media_type().as_bytes()), true);
    assert_eq!(refused(&package(&items)).class(), "kind_mismatch");
}

#[test]
fn a_mimetype_with_a_data_descriptor_is_held_to_the_rest_of_2_3() {
    // The descriptor does not make a compressed entry, one with an extra field or a media type of another kind a `.diacraft` file.
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()).with_local_extra(&[0xCA, 0xFE, 0, 0]), true);
    assert_eq!(sniff(&package(&items)), None);
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", b"application/vnd.oasis.opendocument.graphics"), true);
    assert_eq!(sniff(&package(&items)), None);
    assert_eq!(refused(&package(&items)), NativeError::NotNative);
    // A kind this build does not know starts as a diacraft file does and is refused as one.
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", b"application/vnd.diacraft.scribble"), true);
    let unknown = package(&items);
    assert_eq!(sniff(&unknown), None);
    assert_eq!(refused(&unknown).class(), "kind_unknown");
    // The data is the media type and no more: a descriptor that states more bytes than the type has is a type that is not the kind's, and is not taken for it.
    let mut item = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()), true);
    item.after_data[8..12].copy_from_slice(&(MEDIA_DRAWING.len() as u32 + 3).to_le_bytes());
    item.after_data[12..16].copy_from_slice(&(MEDIA_DRAWING.len() as u32 + 3).to_le_bytes());
    let mut items = vector_1();
    items[0] = item;
    assert_eq!(sniff(&package(&items)), None);
    assert_eq!(refused(&package(&items)).class(), "kind_unknown");
    // A file cut where the descriptor would be is no file of the format.
    let mut items = vector_1();
    items[0] = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()), true);
    let whole = package(&items);
    assert_eq!(sniff(&whole[..38 + MEDIA_DRAWING.len()]), None);
    assert_eq!(sniff(&whole[..38 + MEDIA_DRAWING.len() + 15]), None);
    assert_eq!(sniff(&whole[..38 + MEDIA_DRAWING.len() + 16]), Some(Kind::Drawing));
}

#[test]
fn a_flipped_byte_or_a_cut_in_a_file_with_a_descriptor_never_panics() {
    // The descriptor is read at places that follow the media type, from a file: every byte of the first 120 turned to a few other values, and every cut of them, ends as a kind or
    // as nothing, and as a read or a refusal in words.
    for signature in [true, false] {
        let mut items = vector_1();
        items[0] = with_descriptor(Item::stored("mimetype", MEDIA_DRAWING.as_bytes()), signature);
        let whole = package(&items);
        for at in 0..120usize.min(whole.len()) {
            for value in [0u8, 0xFF, whole[at] ^ 1, whole[at].wrapping_add(1)] {
                let mut bytes = whole.clone();
                bytes[at] = value;
                let _ = sniff(&bytes);
                if let Err(e) = diacraft_native::read(&bytes, &limits()) {
                    assert!(!e.to_string().is_empty(), "words at byte {at}");
                }
            }
            let _ = sniff(&whole[..at]);
        }
    }
}
