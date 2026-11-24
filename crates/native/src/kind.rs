//! The three kinds of `.diacraft` file, the format version this build writes, and the recognition of a file by its `mimetype` entry (the public spec's 1.2
//! and 2.3).
//!
//! **A file is recognised by its bytes, never by its extension.** The first entry of the package is named `mimetype`, is stored, has no extra field, and holds
//! the media type of the file's kind in ASCII with no line end, so that the type is at fixed places in every file: `PK` at 0, `mimetype` at 30 and the media type
//! from 38 (the arrangement OpenDocument's Part 2, 3.3, describes for its own `mimetype`). [`sniff`] reads those places and nothing else, so it costs the same
//! on a file of a kilobyte and one of a gigabyte, and a file manager's magic rule can say the same.

/// The kinds of file: what the package holds (1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A drawing: pages of shapes and the masters they use.
    Drawing,
    /// A set of stencil shapes: masters, and no pages needed.
    Stencil,
    /// A drawing meant to be copied: a program that opens one starts a new, unnamed drawing from it.
    Template,
}

/// The first part of every media type of a `.diacraft` file: a file whose `mimetype` starts with it is recognised as one (2.3).
pub(crate) const MEDIA_TYPE_PREFIX: &str = "application/vnd.diacraft.";

impl Kind {
    /// Every kind, in the order of the spec's table.
    pub const ALL: [Kind; 3] = [Kind::Drawing, Kind::Stencil, Kind::Template];

    /// The media type of the kind: `application/vnd.diacraft.drawing`, `.stencil` or `.template`. They are DiaCraft's own, in the `vnd` tree, and **are not
    /// registered with IANA**.
    pub const fn media_type(self) -> &'static str {
        match self {
            Kind::Drawing => "application/vnd.diacraft.drawing",
            Kind::Stencil => "application/vnd.diacraft.stencil",
            Kind::Template => "application/vnd.diacraft.template",
        }
    }

    /// The file extension of the kind, with its dot: `.diacraft`, `.diacraft-stencil` or `.diacraft-template`. A hint only: the bytes decide.
    pub const fn extension(self) -> &'static str {
        match self {
            Kind::Drawing => ".diacraft",
            Kind::Stencil => ".diacraft-stencil",
            Kind::Template => ".diacraft-template",
        }
    }

    /// The word the manifest's `kind` attribute holds: `drawing`, `stencil` or `template`.
    pub const fn word(self) -> &'static str {
        match self {
            Kind::Drawing => "drawing",
            Kind::Stencil => "stencil",
            Kind::Template => "template",
        }
    }

    /// The kind whose media type is exactly `media_type` (compared byte for byte).
    pub fn from_media_type(media_type: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.media_type() == media_type)
    }

    /// The kind whose manifest word is exactly `word`.
    pub fn from_word(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.word() == word)
    }
}

/// The format version this build writes, `major.minor` (5.1): **1.1**, the version the public spec specifies. A file of a higher minor of major 1 is opened;
/// a file of another major is refused in words.
pub const FORMAT: (u32, u32) = (1, 1);

/// The deflate level every entry of a package is written at: one fixed level, so that the same model gives the same bytes (2.9). A level for speed over size
/// is [`diacraft_container::zip::ArchiveWriter::add_at_level`]'s to take, and a change of it changes every file's bytes and not what they hold.
pub const DEFLATE_LEVEL: u32 = 6;

/// What the first entry of a file says it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sniffed {
    /// Not a `.diacraft` file: the first entry is not a stored `mimetype` with no extra field that starts with [`MEDIA_TYPE_PREFIX`].
    No,
    /// A `.diacraft` file of a kind this build knows.
    Kind(Kind),
    /// A file that starts as a `.diacraft` file does and names a kind this build does not know.
    UnknownKind,
}

/// The bytes at fixed places: the local header's signature, flags, method, sizes, name length and extra length, then the name and the data.
pub(crate) fn sniff_bytes(bytes: &[u8]) -> Sniffed {
    let u16_at = |at: usize| bytes.get(at..at + 2).and_then(|b| <[u8; 2]>::try_from(b).ok()).map(u16::from_le_bytes);
    let u32_at = |at: usize| bytes.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let (Some(signature), Some(flags), Some(method), Some(compressed), Some(size), Some(name_len), Some(extra_len)) =
        (u32_at(0), u16_at(6), u16_at(8), u32_at(18), u32_at(22), u16_at(26), u16_at(28))
    else {
        return Sniffed::No;
    };
    // A stored entry with no extra field, no encryption and a name of exactly `mimetype`, whose bytes are as long as its two sizes say.
    let encrypted = flags & 0x2041 != 0;
    if signature != 0x0403_4B50 || method != 0 || extra_len != 0 || name_len != 8 || encrypted || compressed != size {
        return Sniffed::No;
    }
    if bytes.get(30..38) != Some(b"mimetype".as_slice()) {
        return Sniffed::No;
    }
    // Flag bit 3 says the checksum and the sizes follow the data, in a data descriptor, and the local header holds 0 for them: the length is not at a fixed place, and the media
    // type is told from where it ends (2.3 asks for no more than a stored entry with no extra field, and a streaming writer makes this one).
    if flags & DATA_DESCRIPTOR != 0 && size == 0 {
        return sniff_described(bytes.get(38..).unwrap_or_default());
    }
    let Some(len) = usize::try_from(size).ok().filter(|len| *len <= 256) else { return Sniffed::No };
    let Some(data) = bytes.get(38..38 + len) else { return Sniffed::No };
    let Ok(text) = std::str::from_utf8(data) else { return Sniffed::No };
    if !text.starts_with(MEDIA_TYPE_PREFIX) {
        return Sniffed::No;
    }
    Kind::from_media_type(text).map_or(Sniffed::UnknownKind, Sniffed::Kind)
}

/// Flag bit 3 of a ZIP entry: the checksum and the sizes are in a data descriptor after the data.
const DATA_DESCRIPTOR: u16 = 0x0008;

/// The kind of a `mimetype` whose data (from `rest`'s start) is followed by a data descriptor: the kind whose media type is there **and is followed by a descriptor that states
/// its length** for both sizes, with the descriptor's signature (`PK\x07\x08`) or without it (the checksum, then the two sizes). A text that starts as a media type of this
/// format does and is no kind's with such a descriptor after it names a kind this build does not know.
fn sniff_described(rest: &[u8]) -> Sniffed {
    if !rest.starts_with(MEDIA_TYPE_PREFIX.as_bytes()) {
        return Sniffed::No;
    }
    for kind in Kind::ALL {
        let text = kind.media_type().as_bytes();
        if let Some(after) = rest.strip_prefix(text)
            && descriptor_states(after, text.len())
        {
            return Sniffed::Kind(kind);
        }
    }
    Sniffed::UnknownKind
}

/// Whether `after` starts a data descriptor that states `len` as both its sizes: `PK\x07\x08`, the checksum, the compressed size and the size; or the same without the signature.
fn descriptor_states(after: &[u8], len: usize) -> bool {
    let Ok(len) = u32::try_from(len) else { return false };
    let u32_at = |at: usize| after.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let sizes_at = |at: usize| u32_at(at) == Some(len) && u32_at(at + 4) == Some(len);
    (after.starts_with(&0x0807_4B50u32.to_le_bytes()) && sizes_at(8)) || sizes_at(4)
}

/// The kind of the `.diacraft` file in `bytes`, from the fixed places of its first entry (2.3), or `None` when `bytes` are no file of the format: the first entry
/// is not named `mimetype`, is compressed, has an extra field, or names another type, **whatever the file is called**. A first entry whose checksum and sizes follow its data
/// in a data descriptor (flag bit 3, as a streaming writer makes it) is recognised too, when the descriptor comes right after the media type and states its length. It reads at
/// most 300 bytes. A file that starts as a `.diacraft` file does and names a kind this build does not know is `None` here and is refused in words by [`read`](crate::read).
pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    match sniff_bytes(bytes) {
        Sniffed::Kind(kind) => Some(kind),
        Sniffed::No | Sniffed::UnknownKind => None,
    }
}
