//! Where a save reads the parts of a drawing from, whether the drawing is a package or a flat document.
//!
//! The writer is built on two parts, `content.xml` and `styles.xml`, each scanned and spliced as its own kind ([`super::scan::PartKind`]). A flat drawing holds both
//! in the one document: the document's bytes are the bytes of either part, scanned as the kind that is asked for. [`Source`] is the original a save reads its
//! parts from, and [`Made`] the drawing a save has made, opened to be checked; both give a part by its package name and cost, for a package, what they always did.

use std::borrow::Cow;

use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::package::Opened;

/// The original a save splices into: a package opened, or a flat document's bytes.
pub(crate) enum Source<'s, 'o> {
    Package(&'s mut Opened<'o>),
    Flat(&'s [u8]),
}

/// The bytes of the part `name` of an opened package: `None` when the package has no such file.
fn package_part<'a>(opened: &mut Opened<'_>, name: &str, work: &mut WriteWork) -> Result<Option<Cow<'a, [u8]>>, WriteError> {
    let Some(at) = opened.archive.position(name) else { return Ok(None) };
    let bytes = opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?;
    Ok(Some(Cow::Owned(bytes)))
}

impl<'s> Source<'s, '_> {
    /// Whether the original is a flat document.
    pub(crate) fn is_flat(&self) -> bool {
        matches!(self, Source::Flat(_))
    }

    /// The bytes of `content.xml` or `styles.xml`, inflated for a package and borrowed for a flat document, which is both; `None` when the package has no such
    /// part.
    pub(crate) fn part(&mut self, name: &str, work: &mut WriteWork) -> Result<Option<Cow<'s, [u8]>>, WriteError> {
        match self {
            Source::Package(opened) => package_part(opened, name, work),
            Source::Flat(bytes) => Ok(Some(Cow::Borrowed(*bytes))),
        }
    }
}

/// The package or flat document a save has made, opened for the checks of what it wrote.
pub(crate) enum Made<'b> {
    Package(Box<Opened<'b>>),
    Flat(&'b [u8]),
}

impl<'b> Made<'b> {
    /// Opens `bytes`: a package is opened as a save's own bytes are (the same directory and manifest reads as ever), a flat document is its bytes. A package that
    /// does not open is a fault of the save: `SelfCheck`.
    pub(crate) fn open(bytes: &'b [u8], flat: bool, limits: &Limits, work: &mut WriteWork) -> Result<Made<'b>, WriteError> {
        if flat {
            return Ok(Made::Flat(bytes));
        }
        crate::package::open(bytes, limits, &mut work.package)
            .map(|opened| Made::Package(Box::new(opened)))
            .map_err(|e| WriteError::SelfCheck(format!("{e} ({})", e.class())))
    }

    /// The bytes of the part `name`: for a flat document the whole of it.
    pub(crate) fn part(&mut self, name: &str, work: &mut WriteWork) -> Result<Option<Cow<'b, [u8]>>, WriteError> {
        match self {
            Made::Package(opened) => package_part(opened, name, work),
            Made::Flat(bytes) => Ok(Some(Cow::Borrowed(*bytes))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_document_is_both_parts_and_borrowed() {
        let bytes = b"<office:document/>";
        let mut source = Source::Flat(bytes);
        assert!(source.is_flat());
        for name in ["content.xml", "styles.xml"] {
            let part = source.part(name, &mut WriteWork::default()).unwrap().unwrap();
            assert!(matches!(part, Cow::Borrowed(b) if b == bytes), "{name}: the document itself, not a copy");
        }
        let mut made = Made::open(bytes, true, &Limits::default(), &mut WriteWork::default()).unwrap();
        assert!(matches!(made.part("styles.xml", &mut WriteWork::default()), Ok(Some(Cow::Borrowed(b))) if b == bytes));
    }

    #[test]
    fn bytes_made_as_a_package_that_are_none_are_a_fault_of_the_save() {
        let Err(WriteError::SelfCheck(why)) = Made::open(b"not a package", false, &Limits::default(), &mut WriteWork::default()) else {
            panic!("a package that does not open is a fault of the save");
        };
        assert!(why.contains("not_zip"), "{why}");
    }
}
