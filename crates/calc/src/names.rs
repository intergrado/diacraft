//! Finding a page, master, style or font by the name a formula gives, without searching the document's lists for each reference.
//!
//! `Pages[Site-2]!`, `Masters[Door]!`, `Styles!Callout!` and `FONT("Arial")` name a member of a list the file sets the length of (up to
//! 65,535 masters and styles). A search of the list for each reference costs the list's length, at one step a reference: 26 microseconds a
//! step over 65,536 masters. So each list is listed once, the first time a name in it is asked for, a step and
//! [`NAME_ENTRY_BYTES`] held for each member, and kept for the evaluator's life. A name that two members share is the first one's, as in a
//! search.

use std::collections::HashMap;
use std::sync::Arc;

use crate::eval::Evaluator;
use crate::rules::NAME_ENTRY_BYTES;

type Names = HashMap<Arc<str>, u32>;

/// The lists, each built on first use.
#[derive(Default)]
pub(crate) struct DocNames {
    /// A page by `NameU` or by `Name`.
    pages_any: Option<Names>,
    /// A page by `NameU` only (a name that is a number reads this).
    pages_u: Option<Names>,
    /// A master by `NameU` or by `Name`.
    masters_any: Option<Names>,
    /// A master by `NameU` or by its `UniqueID`.
    masters_theme: Option<Names>,
    /// A style by `NameU`.
    styles_u: Option<Names>,
    /// A font by its `NameU` in lower case (ASCII), to its place in the face list.
    fonts: Option<HashMap<String, usize>>,
}

impl Evaluator<'_, '_> {
    /// Charges the listing of `n` members: a step for each and the memory their entries hold. False when the budget is spent.
    fn charge_listing(&mut self, n: usize) -> bool {
        let n = n as u64;
        self.ctx.take_steps(n.max(1)) && self.ctx.hold_bytes(n.saturating_mul(NAME_ENTRY_BYTES))
    }

    /// The id of the first page named `name`: by `NameU` or `Name` when `plain` is true, by `NameU` alone when it is false.
    pub(crate) fn page_named(&mut self, name: &str, plain: bool) -> Option<u32> {
        let doc = self.doc;
        let built = if plain { self.names.pages_any.is_some() } else { self.names.pages_u.is_some() };
        if !built {
            if !self.charge_listing(doc.pages.len()) {
                return None;
            }
            let mut map = Names::with_capacity(doc.pages.len());
            for p in doc.pages.iter() {
                if let Some(n) = &p.name_u {
                    map.entry(Arc::clone(n)).or_insert(p.id);
                }
                if plain {
                    map.entry(Arc::clone(&p.name)).or_insert(p.id);
                }
            }
            if plain {
                self.names.pages_any = Some(map);
            } else {
                self.names.pages_u = Some(map);
            }
        }
        let map = if plain { self.names.pages_any.as_ref() } else { self.names.pages_u.as_ref() };
        map?.get(name).copied()
    }

    /// The id of the first master named `name` by `NameU` or `Name`.
    pub(crate) fn master_named(&mut self, name: &str) -> Option<u32> {
        let doc = self.doc;
        if self.names.masters_any.is_none() {
            if !self.charge_listing(doc.masters.len()) {
                return None;
            }
            let mut map = Names::with_capacity(doc.masters.len());
            for m in doc.masters.iter() {
                for n in [&m.name_u, &m.name].into_iter().flatten() {
                    map.entry(Arc::clone(n)).or_insert(m.id);
                }
            }
            self.names.masters_any = Some(map);
        }
        self.names.masters_any.as_ref()?.get(name).copied()
    }

    /// The id of the first master whose `NameU` or `UniqueID` is `name` (`THEME`'s custom scheme names a master by either).
    pub(crate) fn master_named_or_unique(&mut self, name: &str) -> Option<u32> {
        let doc = self.doc;
        if self.names.masters_theme.is_none() {
            if !self.charge_listing(doc.masters.len()) {
                return None;
            }
            let mut map = Names::with_capacity(doc.masters.len());
            for m in doc.masters.iter() {
                for n in [&m.name_u, &m.unique_id].into_iter().flatten() {
                    map.entry(Arc::clone(n)).or_insert(m.id);
                }
            }
            self.names.masters_theme = Some(map);
        }
        self.names.masters_theme.as_ref()?.get(name).copied()
    }

    /// The id of the first style whose `NameU` is `name`.
    pub(crate) fn style_named(&mut self, name: &str) -> Option<u32> {
        let doc = self.doc;
        if self.names.styles_u.is_none() {
            if !self.charge_listing(doc.styles.len()) {
                return None;
            }
            let mut map = Names::with_capacity(doc.styles.len());
            for s in doc.styles.iter() {
                if let Some(n) = &s.name_u {
                    map.entry(Arc::clone(n)).or_insert(s.id);
                }
            }
            self.names.styles_u = Some(map);
        }
        self.names.styles_u.as_ref()?.get(name).copied()
    }

    /// The place in the document's face list of the first face whose name is `name`, the case of ASCII letters ignored. The name is charged
    /// by its length, as it is folded to look up.
    pub(crate) fn font_place(&mut self, name: &str) -> Option<usize> {
        let doc = self.doc;
        if self.names.fonts.is_none() {
            if !self.charge_listing(doc.fonts.len()) {
                return None;
            }
            let mut map: HashMap<String, usize> = HashMap::with_capacity(doc.fonts.len());
            for (at, f) in doc.fonts.iter().enumerate() {
                map.entry(f.name_u.to_ascii_lowercase()).or_insert(at);
            }
            self.names.fonts = Some(map);
        }
        if !self.work(name.len()) {
            return None;
        }
        self.names.fonts.as_ref()?.get(&name.to_ascii_lowercase()).copied()
    }
}
