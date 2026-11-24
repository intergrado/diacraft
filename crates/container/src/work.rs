//! Counts of the work a read does, each in the units its loop walks, for the tests that hold a cost to a count: a count does not move
//! with the load on the machine, as a time does, and a ratio of two counts is exact where a ratio of two times is not.
//!
//! A [`Work`] is passed by the caller and added to, so that it says what a read cost when the read is refused as well as when it reads:
//! the refusals are where a hostile file spends what it can. The counts are plain additions on the paths they count, with no
//! allocation, so they are always on and the app carries them too; nothing reads them but a caller that asks.

/// What reading an archive or an XML tree cost, so far. Every field only grows; [`Work::default`] is all zeros.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Work {
    /// Positions the backward scan for the end record looked at: at most 65,536, the window of the record's 22 bytes and its longest
    /// comment, however long the file is and however many signatures it holds.
    pub end_scan: u64,
    /// Rows of the central directory read. Refused from the count, a directory costs none.
    pub rows: u64,
    /// Records of the rows' extra fields walked: each once, and none kept.
    pub extra_records: u64,
    /// Records of the extra fields walked by a copy of an entry ([`crate::zip::ArchiveWriter::copy_counted`], [`crate::zip::copy_account_counted`]): the
    /// local header's, looked through for its ZIP64 record, and the local header's and the row's again when an entry is written under headers made
    /// anew. Each walk of each entry is counted, so a copy and the account of it say what an area of many records cost.
    pub local_extra_records: u64,
    /// Bytes of names read out of the rows: each name as written, and each Unicode path field's name.
    pub name_bytes: u64,
    /// Comparisons of two names that the plain judge's duplicate rule made ([`crate::zip::Plain`]): those of its sort and those of its pass
    /// over neighbours. A comparison, not a byte: each looks at no more than the shorter name, at most the name limit. A judge of a caller's
    /// may add its own here.
    pub name_comparisons: u64,
    /// Local headers read: one for each row, to lay the entries out, and one more for each entry kept.
    pub local_headers: u64,
    /// Bytes that came out of entries read (inflated, or copied when stored), including the byte past a limit that refuses one.
    pub inflated: u64,
    /// Events the XML parser read: tags, text, references, CDATA, comments, instructions, declarations.
    pub xml_events: u64,
    /// Events the tag scan read ([`crate::xml_scan::scan_tags`]) where its caller counts them apart from the tree reader's: the scan of a
    /// part for the files it names, which a read does to leave the others uninflated.
    pub scan_events: u64,
    /// Units the content check ([`crate::canon`]) walks, each counted once: an element put into its canonical form, an attribute, an item or a
    /// keyed child met by the comparison, and a comparison made by one of its sorts or merges. A count of work, so that a test can hold the check
    /// to a multiple of the size of its input (`n log n` at the most) and not to a time.
    pub canon_steps: u64,
    /// Bytes the content check looked at: the values and the text it put into canonical form, the shorter of two values or two keys it set
    /// side by side, the bytes of two parts it compared as bytes, and the bytes of a name or a key it sorted by. The steps count how many
    /// things were compared; these count how long they were, so that long equal keys cannot hide behind a small step count.
    pub canon_bytes: u64,
}

impl Work {
    /// Adds `n` to a count, without wrapping.
    pub(crate) fn add(count: &mut u64, n: usize) {
        *count = count.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
    }
}
