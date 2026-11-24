//! A style set's strings shown in cells: a font's family, a bullet's characters, made once and shared by every cell that shows them.
//!
//! **Where a value is kept.** A family named by many styles must not be copied into every conversion (4,000
//! styles naming a family of a million characters aborted). So each text is made into one [`Value`] the first time it is shown, charged by
//! its bytes then, and the cells share it. To find that value again in one step whatever the text's length, [`StyleTexts`] keys it by where
//! the text is held, its address and length, and not by its content, which would hash the whole text on every call and bring the cost back.
//!
//! **Why that key is sound, by the type and not by a promise.** An address names a text only while that text stays where it is: not freed,
//! not moved, and its place not given to another string. 's MEDIUM was a text from a temporary clone, freed when
//! the list that held it ended, whose address the next list's clone took, so that one list showed another's bullet. Here a cache is made
//! from a shared borrow of the [`StyleSet`] for a lifetime `'s`, and takes only `&'s str`: text borrowed for as long as the cache lives. A
//! string that is freed, moved or changed while the cache is alive cannot be borrowed so long, and the set cannot be changed while it is
//! borrowed. A text from a temporary does not compile, and neither does a text from the set once the set is let go of.
//!
//! **One cache for each stretch of the read that holds the set still.** The set grows during the read (`content.xml`'s styles join it after
//! the style sheets of `styles.xml` are built), so no single borrow lasts the whole read: the root and the style sheets, the master pages'
//! shapes and the pages' shapes each make a cache of their own. The values themselves are shared across them, by the content hash of the
//! text ([`Reader::shared_value`]), so a family shown in two stretches is still one value, charged once.

use std::collections::HashMap;
use std::marker::PhantomData;

use diacraft_doc::Value;

use super::Reader;
use super::styles::{File, StyleSet};
use crate::error::OdgError;

/// The values of a style set's texts, by where each text is held, for as long as the set is borrowed (`'s`). See the module documentation.
pub(crate) struct StyleTexts<'s> {
    set: &'s StyleSet,
    /// Each text's value, by the text's address and length.
    values: HashMap<(usize, usize), Value>,
    /// The family each face name finds, by where the name is held and the file it is looked up from: a name is hashed once, not once for
    /// every cell that names it.
    faces: HashMap<(usize, usize, File), Option<&'s str>>,
    /// Makes `'s` invariant, so that a cache cannot be taken for one of a shorter lifetime and given a text that does not last.
    held: PhantomData<fn(&'s ()) -> &'s ()>,
}

impl<'s> StyleTexts<'s> {
    /// A cache for the texts `set` holds while it is borrowed.
    pub(crate) fn of(set: &'s StyleSet) -> StyleTexts<'s> {
        StyleTexts { set, values: HashMap::new(), faces: HashMap::new(), held: PhantomData }
    }

    /// The value of `text`, a text the style set holds (a font's family, a bullet's characters): made and charged the first time its place
    /// is seen, and the same value, in one step, every time after. Equal texts held in two places are one value (`Reader::shared_value`).
    pub(crate) fn value(&mut self, r: &mut Reader, text: &'s str) -> Result<Value, OdgError> {
        let at = (text.as_ptr() as usize, text.len());
        if let Some(found) = self.values.get(&at) {
            return Ok(found.clone());
        }
        let value = r.shared_value(text)?;
        self.values.insert(at, value.clone());
        Ok(value)
    }

    /// The family the font face `name` declares for a style of `file` (`StyleSet::font_family`), looked up once for each place a name is
    /// held: a style whose face name is a million characters long, shown by many runs, is not hashed again for each. The bytes of each name
    /// looked up are counted ([`crate::ReadWork::name_bytes`]).
    pub(crate) fn face(&mut self, r: &mut Reader, name: &'s str, file: File) -> Result<Option<&'s str>, OdgError> {
        let at = (name.as_ptr() as usize, name.len(), file);
        if let Some(found) = self.faces.get(&at) {
            return Ok(*found);
        }
        r.take_items(1)?;
        r.name_bytes(name.len());
        let found = self.set.font_family(name, file);
        self.faces.insert(at, found);
        Ok(found)
    }
}
