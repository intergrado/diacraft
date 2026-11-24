//! The kinds of picture DiaCraft draws, told by the picture's own first bytes: **one table for the reader and the writer**. The reader judges
//! a picture by it (its content type, and the `ForeignType` it gives the shape), and the writer types a new picture by it (the manifest row's media type, and the extension of the
//! entry's name). Two tables that had to be kept in step by hand were two chances for a picture the writer writes to read back as another kind, or as none (the review
//!
//! A picture is judged by its bytes and never by its name or by the media type a file says: PNG (0x89 `PNG`), JPEG (0xFF 0xD8), GIF (`GIF8`) and BMP (`BM`) are bitmaps, and EMF
//! (` EMF` at bytes 40 to 43) is an enhanced metafile. Another format (SVG, WMF and the rest) is none of these, and the reader keeps it as a placeholder.
//!
//! **An EMF is typed `image/emf`** (owner ruling C2, 2026-11-17): the media type [RFC 7903](https://www.rfc-editor.org/rfc/rfc7903.html) (Windows Image
//! Media Types, read 2026-11-17) registers for it. The `image/x-emf` that files made by other programs carry is the older, unregistered spelling: **a row that says it is
//! kept as the file has it** (the writer never rewrites a row of an entry it does not write), and **the reader takes both** because it judges a picture by its bytes
//! and never by the type a row gives.

/// The name a picture read from a flat drawing's `office:binary-data` has as the model's `Media::part`: **no entry of any package**, so that a writer never names an entry or a link by
/// it (a flat drawing saved as a package wrote the picture as an entry called `office:binary-data` and linked it by that name, and two pictures were one name). A package
/// the writer makes names a picture by its own entry, `Pictures/image<n>.<ext>`.
pub(crate) const INLINE_PART: &str = "office:binary-data";

/// What a picture's bytes say it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PictureKind {
    /// The media type, as a manifest row gives it.
    pub media: &'static str,
    /// The extension of a new entry's name.
    pub ext: &'static str,
    /// The `ForeignType` the shape a frame of this picture reads as has.
    pub foreign: &'static str,
}

/// The five kinds, in the order the signatures are tried.
pub(crate) const KINDS: [PictureKind; 5] = [
    PictureKind { media: "image/png", ext: "png", foreign: "Bitmap" },
    PictureKind { media: "image/jpeg", ext: "jpg", foreign: "Bitmap" },
    PictureKind { media: "image/gif", ext: "gif", foreign: "Bitmap" },
    PictureKind { media: "image/bmp", ext: "bmp", foreign: "Bitmap" },
    PictureKind { media: "image/emf", ext: "emf", foreign: "EnhMetaFile" },
];

/// The kind of a picture by its first bytes; `None` for a format DiaCraft does not draw.
pub(crate) fn picture_kind(bytes: &[u8]) -> Option<PictureKind> {
    let at = match bytes {
        [0x89, b'P', b'N', b'G', ..] => 0,
        [0xFF, 0xD8, ..] => 1,
        [b'G', b'I', b'F', b'8', ..] => 2,
        [b'B', b'M', ..] => 3,
        b if b.get(40..44) == Some(b" EMF") => 4,
        _ => return None,
    };
    KINDS.get(at).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each signature by itself, its near misses, and every kind of the table reached by exactly one signature (so the table and the match cannot drift apart).
    #[test]
    fn each_kind_is_told_by_its_signature_and_by_nothing_else_and_the_table_holds_each_once() {
        let mut emf = vec![0u8; 40];
        emf.extend_from_slice(b" EMF");
        let heads: [(&[u8], &str); 5] = [
            (&[0x89, b'P', b'N', b'G', 1], "image/png"),
            (&[0xFF, 0xD8, 0], "image/jpeg"),
            (b"GIF89a", "image/gif"),
            (b"BM\0\0", "image/bmp"),
            (&emf, "image/emf"),
        ];
        for (bytes, media) in heads {
            assert_eq!(picture_kind(bytes).map(|k| k.media), Some(media), "{media}");
        }
        let reached: Vec<&str> = heads.iter().filter_map(|(b, _)| picture_kind(b)).map(|k| k.media).collect();
        let table: Vec<&str> = KINDS.iter().map(|k| k.media).collect();
        assert_eq!(reached, table, "the signatures reach the table's kinds in its order, each once");
        let mut late = vec![0u8; 41];
        late.extend_from_slice(b" EMF");
        for none in [&b""[..], b"GIF", b"B", &[0xFF, 0xD7], &[0x89, b'P', b'N'], b"<svg xmlns=", &[0xD7, 0xCD, 0xC6, 0x9A, 0, 0], &late, &emf[..43]] {
            assert_eq!(picture_kind(none), None, "{none:?}");
        }
        assert!(KINDS.iter().all(|k| matches!(k.foreign, "Bitmap" | "EnhMetaFile")));
        assert_eq!(picture_kind(&emf).map(|k| (k.ext, k.foreign)), Some(("emf", "EnhMetaFile")));
        assert_eq!(picture_kind(&[0xFF, 0xD8]).map(|k| k.ext), Some("jpg"));
    }
}
