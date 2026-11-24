//! The sheets and sections of a part that are equal **by value** (item 2; the vocabulary's 3.19, format 1.1).
//!
//! A drawing that comes from a reader that shares nothing holds each equal sheet and section as an allocation of its own: the `.vsdx` reader's does (a path of five rows is
//! five rows in every shape of a thousand), and so does a model after a paste or an edit that happened to make two units alike. The encoder shares by the address of the
//! allocation ([`super::encode`]'s `Units`); this finds the units that are equal and are not one allocation, and gives each **class** of them one name, so that the file says a
//! unit once (a `def`) and each repeat as a `same`, whichever way the model came to hold them.
//!
//! **No hash is the proof of equality**. The key of a unit is a hash of what it holds that tells units apart (the names, the values, the formulas and the units of its
//! cells, its links, and the class of each section it holds), and it **only finds candidates**: a unit that has the key of one met before is compared with it, in full, by the
//! model's own `==`, and is in its class when that says so and in a class of its own when it does not. A key holds at most [`BUCKET_MAX`] different units, so a part made of
//! units that all have one key costs a few comparisons for each unit and not as many as there are units; the units beyond are each their own class (never wrongly one: a unit is
//! put in a class only when it is equal to the one that names it). The classes are therefore sound and not complete: two equal units may be two classes, and two classes are
//! never one unit.
//!
//! A sheet is equal to another when its links, cells, raw attributes, kept nodes and slot are equal and its sections are, one by one, in the same classes (which is equal sections,
//! and costs a comparison of the classes and not of the rows again).
//!
//! The classes are made once for each part, by a walk of its shapes with a stack of its own, before the part is made, and the allocations are visited once each (the address of a
//! unit that many shapes hold is looked up and not hashed again).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use diacraft_doc::{Cells, Row, Section, ShapeList, Sheet};

use super::share::BUCKET_MAX;

/// A sheet met before: the address that names its class, the sheet, and the classes of its sections.
struct SheetEntry<'a> {
    rep: usize,
    sheet: &'a Sheet,
    sections: Box<[usize]>,
}

/// The classes of the units of a part: for each allocation, the address of the first allocation equal to it, and for each class how many places hold it.
pub(super) struct Classifier<'a> {
    /// The class of every unit met, by its address.
    by_addr: HashMap<usize, usize>,
    sheets: HashMap<u64, Vec<SheetEntry<'a>>>,
    sections: HashMap<u64, Vec<(usize, &'a Section)>>,
    /// The places that hold each class.
    counts: HashMap<usize, u32>,
    /// Every key is the same, so that the comparison is all that tells units apart (the tests).
    #[cfg(test)]
    collide: bool,
}

/// What a part's units come to: the class of each allocation that is not the first of its class, and the classes held in more than one place.
pub(super) struct Classes {
    /// An allocation's class, when it is not its own.
    pub(super) class: HashMap<usize, usize>,
    /// The classes that two or more places hold, with how many.
    pub(super) held: HashMap<usize, u32>,
}

impl<'a> Classifier<'a> {
    pub(super) fn new() -> Classifier<'a> {
        Classifier {
            by_addr: HashMap::new(),
            sheets: HashMap::new(),
            sections: HashMap::new(),
            counts: HashMap::new(),
            #[cfg(test)]
            collide: false,
        }
    }

    #[cfg(test)]
    pub(super) fn colliding() -> Classifier<'a> {
        Classifier { collide: true, ..Classifier::new() }
    }

    fn key(&self, hash: u64) -> u64 {
        #[cfg(test)]
        if self.collide {
            return 0;
        }
        hash
    }

    /// Notes a place that holds the sheet `s`, and each of its sections.
    pub(super) fn place(&mut self, s: &'a Arc<Sheet>) {
        let rep = self.sheet(s);
        let count = self.counts.entry(rep).or_insert(0);
        *count = count.saturating_add(1);
        for section in &s.sections {
            let rep = self.section(section);
            let count = self.counts.entry(rep).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    /// Notes the places of every shape of `lists` and of each shape under them, by a walk with a stack of its own.
    pub(super) fn shapes(&mut self, lists: &[&'a ShapeList]) {
        let mut stack: Vec<&'a ShapeList> = lists.to_vec();
        while let Some(list) = stack.pop() {
            for shape in list {
                self.place(&shape.sheet);
                if !shape.children.is_empty() {
                    stack.push(&shape.children);
                }
            }
        }
    }

    /// The class of a section: the address of the first allocation equal to it.
    fn section(&mut self, s: &'a Arc<Section>) -> usize {
        let addr = Arc::as_ptr(s).addr();
        if let Some(rep) = self.by_addr.get(&addr) {
            return *rep;
        }
        let key = self.key(section_key(s));
        let section: &'a Section = s;
        let bucket = self.sections.entry(key).or_default();
        // The comparison is the proof: the same class when `==` says the same, and never because the keys agree.
        let rep = match bucket.iter().find(|(_, other)| *other == section) {
            Some((rep, _)) => *rep,
            None => {
                if bucket.len() < BUCKET_MAX {
                    bucket.push((addr, section));
                }
                addr
            }
        };
        self.by_addr.insert(addr, rep);
        rep
    }

    /// The class of a sheet: the address of the first allocation equal to it.
    fn sheet(&mut self, s: &'a Arc<Sheet>) -> usize {
        let addr = Arc::as_ptr(s).addr();
        if let Some(rep) = self.by_addr.get(&addr) {
            return *rep;
        }
        let sections: Box<[usize]> = s.sections.iter().map(|section| self.section(section)).collect();
        let key = self.key(sheet_key(s, &sections));
        let sheet: &'a Sheet = s;
        let bucket = self.sheets.entry(key).or_default();
        let rep = match bucket.iter().find(|other| equal_sheets(other.sheet, &other.sections, sheet, &sections)) {
            Some(other) => other.rep,
            None => {
                if bucket.len() < BUCKET_MAX {
                    bucket.push(SheetEntry { rep: addr, sheet, sections });
                }
                addr
            }
        };
        self.by_addr.insert(addr, rep);
        rep
    }

    /// The classes, once every place is noted.
    pub(super) fn finish(self) -> Classes {
        let class = self.by_addr.into_iter().filter(|(addr, rep)| addr != rep).collect();
        let held = self.counts.into_iter().filter(|(_, n)| *n >= 2).collect();
        Classes { class, held }
    }
}

/// Whether two sheets are equal: everything but their sections by `==`, and their sections by class (which is equal sections: the same number of them, each in the class of the
/// one in its place).
pub(crate) fn equal_sheets(a: &Sheet, a_sections: &[usize], b: &Sheet, b_sections: &[usize]) -> bool {
    a_sections == b_sections && a.links == b.links && a.cells == b.cells && a.raw == b.raw && a.passthrough == b.passthrough && a.ext == b.ext
}

fn hash_cells(cells: &Cells, h: &mut DefaultHasher) {
    cells.len().hash(h);
    for (name, cell) in cells {
        name.as_str().hash(h);
        cell.formula.text().hash(h);
        cell.value.as_ref().map(diacraft_doc::Value::text).hash(h);
        cell.unit.as_deref().hash(h);
        cell.error.as_deref().hash(h);
        cell.extra.is_some().hash(h);
    }
}

fn hash_row(row: &Row, h: &mut DefaultHasher) {
    row.name.as_deref().hash(h);
    row.ix.hash(h);
    row.local_name.as_deref().hash(h);
    row.row_type.as_deref().hash(h);
    row.del.hash(h);
    hash_cells(&row.cells, h);
}

pub(crate) fn section_key(s: &Section) -> u64 {
    let mut h = DefaultHasher::new();
    s.name.as_str().hash(&mut h);
    s.ix.hash(&mut h);
    s.del.hash(&mut h);
    hash_cells(&s.cells, &mut h);
    s.rows.len().hash(&mut h);
    for row in &s.rows {
        hash_row(row, &mut h);
    }
    h.finish()
}

pub(crate) fn sheet_key(s: &Sheet, sections: &[usize]) -> u64 {
    let mut h = DefaultHasher::new();
    s.links.line.hash(&mut h);
    s.links.fill.hash(&mut h);
    s.links.text.hash(&mut h);
    hash_cells(&s.cells, &mut h);
    sections.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use diacraft_doc::{Cell, Value};

    use super::*;

    fn section(y: &str) -> Arc<Section> {
        let mut s = Section::new("Geometry", Some(0));
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert("Y".into(), Cell::constant(Value::from_text(y)));
        s.rows.push(Arc::new(row));
        Arc::new(s)
    }

    fn sheet_with(sections: Vec<Arc<Section>>) -> Arc<Sheet> {
        Arc::new(Sheet { sections, ..Sheet::default() })
    }

    #[test]
    fn the_comparison_and_not_the_key_decides_when_every_key_agrees() {
        // Eight different sections and their repeats, all under one key: each repeat is in the class of its own twin and in no other's, as far as the key has room, and no
        // class ever holds two sections that differ.
        let originals: Vec<Arc<Section>> = (0..8).map(|i| section(&i.to_string())).collect();
        let twins: Vec<Arc<Section>> = (0..8).map(|i| section(&i.to_string())).collect();
        let mut classes = Classifier::colliding();
        let sheets: Vec<Arc<Sheet>> = originals.iter().chain(&twins).map(|s| sheet_with(vec![Arc::clone(s)])).collect();
        for s in &sheets {
            classes.place(s);
        }
        let by = |s: &Arc<Section>| {
            let addr = Arc::as_ptr(s).addr();
            classes.by_addr.get(&addr).copied().unwrap()
        };
        // Whatever the class of two sections, they are equal when the classes are.
        let all: Vec<&Arc<Section>> = originals.iter().chain(&twins).collect();
        for a in &all {
            for b in &all {
                if by(a) == by(b) {
                    assert_eq!(***a, ***b, "one class, two different sections");
                }
            }
        }
        // The first four distinct units (the room of a key) are shared with their twins; the others are not entered and are each their own class.
        for i in 0..BUCKET_MAX {
            assert_eq!(by(&originals[i]), by(&twins[i]), "unit {i}");
        }
        for i in BUCKET_MAX..8 {
            assert_ne!(by(&originals[i]), by(&twins[i]), "unit {i} was past the key's room");
        }
    }

    #[test]
    fn two_sheets_are_equal_only_when_their_raw_attributes_and_their_kept_nodes_are() {
        // The attributes a sheet's element had that the model has no field for, and the nodes it kept, are part of what it is. Each is a guard of its own:
        // two sheets that differ only in one of them are two sheets, though their keys (which do not hash them) agree.
        let plain = Sheet::default();
        let mut raw = Sheet::default();
        raw.raw.push("UniqueID", "{1}");
        let mut kept = Sheet::default();
        kept.passthrough.push(0, diacraft_doc::RawNode::Text(Arc::from("a node of a newer build")));
        let none: &[usize] = &[];
        assert!(equal_sheets(&plain, none, &Sheet::default(), none), "equal in everything");
        assert!(!equal_sheets(&plain, none, &raw, none), "a raw attribute tells them apart");
        assert!(!equal_sheets(&raw, none, &plain, none));
        assert!(!equal_sheets(&plain, none, &kept, none), "a kept node tells them apart");
        assert!(!equal_sheets(&kept, none, &plain, none));
        assert!(equal_sheets(&raw, none, &raw.clone(), none));
        assert!(equal_sheets(&kept, none, &kept.clone(), none));
        // And a different class of sections is another sheet.
        assert!(!equal_sheets(&plain, &[1], &Sheet::default(), &[2]));
        assert_eq!(sheet_key(&plain, none), sheet_key(&raw, none), "the key does not see them: the comparison does");
    }

    #[test]
    fn sheets_are_told_apart_by_comparison_when_every_key_agrees() {
        let one = section("1");
        let two = section("2");
        let mut classes = Classifier::colliding();
        let (a, b, c) = (sheet_with(vec![Arc::clone(&one)]), sheet_with(vec![Arc::clone(&two)]), sheet_with(vec![section("1")]));
        for s in [&a, &b, &c] {
            classes.place(s);
        }
        let class = |s: &Arc<Sheet>| classes.by_addr[&Arc::as_ptr(s).addr()];
        assert_eq!(class(&a), class(&c), "equal sections, equal sheets");
        assert_ne!(class(&a), class(&b));
        let done = classes.finish();
        assert_eq!(done.held.values().copied().max(), Some(2));
    }
}
