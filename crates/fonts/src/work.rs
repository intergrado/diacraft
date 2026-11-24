//! Counts of the work the font code does on hostile input, for the tests that hold a cost to a count: a count does not move with the load
//! on the machine, as a time does, and it tells a record read in full from one read to its sixth character where a time cannot.
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls that
//! bump one compile to nothing. The counts are of the calling thread, and the font code works on the thread that calls it.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static LANGUAGE_TAGS: Cell<u64> = const { Cell::new(0) };
        static LANGUAGE_TAG_CHARS: Cell<u64> = const { Cell::new(0) };
        static THIS_TAG_CHARS: Cell<u64> = const { Cell::new(0) };
        static LONGEST_TAG_READ: Cell<u64> = const { Cell::new(0) };
        static FILE_BYTES_READ: Cell<u64> = const { Cell::new(0) };
        static PEAK_PATH_ELEMENTS: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        LANGUAGE_TAGS.with(|c| c.set(0));
        LANGUAGE_TAG_CHARS.with(|c| c.set(0));
        THIS_TAG_CHARS.with(|c| c.set(0));
        LONGEST_TAG_READ.with(|c| c.set(0));
        FILE_BYTES_READ.with(|c| c.set(0));
        PEAK_PATH_ELEMENTS.with(|c| c.set(0));
    }

    /// The `name` records whose language was read from a language tag of the table.
    #[must_use]
    pub fn language_tags() -> u64 {
        LANGUAGE_TAGS.with(Cell::get)
    }

    /// The characters of those tags that were looked at.
    #[must_use]
    pub fn language_tag_chars() -> u64 {
        LANGUAGE_TAG_CHARS.with(Cell::get)
    }

    /// The most characters read from any one tag.
    #[must_use]
    pub fn longest_tag_read() -> u64 {
        LONGEST_TAG_READ.with(Cell::get)
    }

    /// The bytes read from font files to catalogue them (a range read before is not read again, and does not count again).
    #[must_use]
    pub fn file_bytes_read() -> u64 {
        FILE_BYTES_READ.with(Cell::get)
    }

    /// The most path elements one glyph's outline held at once while it was drawn.
    #[must_use]
    pub fn peak_path_elements() -> u64 {
        PEAK_PATH_ELEMENTS.with(Cell::get)
    }

    pub(crate) fn language_tag() {
        LANGUAGE_TAGS.with(|c| c.set(c.get().saturating_add(1)));
        THIS_TAG_CHARS.with(|c| c.set(0));
    }

    pub(crate) fn language_tag_char() {
        LANGUAGE_TAG_CHARS.with(|c| c.set(c.get().saturating_add(1)));
        let this = THIS_TAG_CHARS.with(|c| {
            c.set(c.get().saturating_add(1));
            c.get()
        });
        LONGEST_TAG_READ.with(|c| c.set(c.get().max(this)));
    }

    pub(crate) fn file_bytes(n: u64) {
        FILE_BYTES_READ.with(|c| c.set(c.get().saturating_add(n)));
    }

    pub(crate) fn path_elements_held(n: usize) {
        PEAK_PATH_ELEMENTS.with(|c| c.set(c.get().max(n as u64)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{file_bytes, language_tag, language_tag_char, path_elements_held};
#[cfg(feature = "test-support")]
pub use on::{file_bytes_read, language_tag_chars, language_tags, longest_tag_read, peak_path_elements, reset};

#[cfg(not(feature = "test-support"))]
pub(crate) fn language_tag() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn language_tag_char() {}
#[cfg(all(not(feature = "test-support"), not(target_arch = "wasm32")))]
pub(crate) fn file_bytes(_: u64) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn path_elements_held(_: usize) {}
