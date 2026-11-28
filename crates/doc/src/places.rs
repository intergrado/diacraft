//! Where the unknown nodes of a sheet stand: the places that **every writer** of a cell, a row or a section keeps right.
//!
//! The model keeps a node it has no field for (a foreign element, a comment, an instruction) as **the count of typed children that stood before it**
//! ([`Placed::after`](crate::Placed)), and a writer that makes the file from the model puts the node back after that many (the `.diacraft` writer keeps the count as it is; the
//! model writer of a VSDX file, a Save As across the formats, weaves the node among the children it writes). A change that puts a row before the node, or takes one out, has to move the
//! count with it, or the node stands one place off in the file a model writer makes: a comment that stood after row 2 stands after row 1, or row 3.
//!
//! **What counts what.** The nodes of a *row* count its cells; those of a *section* count its cells and then its rows; those of a *sheet* count its cells and then its sections; and the
//! nodes of the shape or the style that holds the sheet's cells and sections among its own children ([`Shape::passthrough`](crate::Shape), [`Style::passthrough`](crate::Style); the
//! `Text` and the members of a shape come after the sections) count the same children, so **a section that goes in or out moves the places of two lists**, the sheet's and the
//! shape's or the style's ([`Counted`]). A page's, a master's and the document's sheet is an element of its own, and its nodes are the sheet's.
//!
//! **The rule is the one a save into the file keeps** (`diacraft_sheetxml::patch`, which keeps every node attached to the item before it): a node goes on following the child it
//! followed, ahead of whatever is put in right after that child, and when the child it followed goes it follows the one before. So **a new row goes ahead of a node that stood
//! after the row before it**, at the end of a section too, and behind a node that stood before the first row; a new section goes ahead of the nodes that stood right after the cells; a
//! new cell goes ahead of the nodes that stood after the cell before it. The two kinds of save then write the node in the same place
//! ([`Passthrough::children_inserted`] and [`Passthrough::children_removed`] do the arithmetic).
//!
//! **Who keeps them.** The writers of this crate that make a section, a row or a cell ([`Sheet::section_mut`], [`Sheet::row_mut`], [`Sheet::set_cell`], [`Sheet::remove_cell`], and
//! [`Row::put_cell`], [`Section::put_cell`], [`Section::insert_row`], [`Section::remove_rows`], [`Section::retain_rows`], [`Sheet::insert_section`], [`Sheet::remove_sections`]) move the sheet's, the section's and the row's own
//! nodes themselves, so **no writer that goes through them can forget**. The nodes of the shape or the style that holds the sheet are not the sheet's: [`Counted`] (got from
//! [`Document::counted_mut`], [`Shape::counted_sheet`] and [`Style::counted_sheet`]) has the same writers and moves those too, and a writer that holds a shape or a style uses it.
//!
//! **Lists of shapes.** The unknown nodes of the `Shapes` element of a page, a master or a group are kept in the slot of their owner (the `Shapes` entry of `ext`), counted by the
//! shapes before them, and move by the same rule: [`Page::push_shape`], [`Page::insert_shape`] and [`Page::remove_shape`] (and the same for a master and for the members of a group:
//! [`Shape::push_member`] and its kin) move them with the shapes they followed at the cost of the nodes held, and [`Page::edit_shapes`] does it for any change of the list (a new drawing
//! order, a group, an ungroup) by telling the shapes apart by their `ID` ([`Passthrough::children_followed`]).
//!
//! **Costs, as counts.** Nothing is done for a list with no node, which is nearly every list: the lists are asked first. A change is one pass over the nodes of the list it reaches (a
//! place is found by a search over the places that were put in, not by a pass for each), counted by [`crate::work::places_read`]; a list of shapes that is rebuilt reads its keys once
//! each besides ([`crate::work::follow_keys`]).
//!
//! **Lists made again.** A command that puts a set of rows in place of a section's own (a connector's route, a path a tool carries on) uses [`Section::replace_rows`]: the
//! rows are told by their key (`IX` or `N`), and each node goes on following the row of the key it followed. A sheet, a section or a row that is **built again from another** (the sheet of a
//! shape changed to another master, the copy a Save As an `.odg` is given) takes the nodes of the old one with [`Sheet::take_places_of`] (and [`Sheet::take_places_of_where`] when some of the
//! old sections are replaced by the new master's), [`Section::take_places_of`] and [`owner_places_after`], which tell the cells by their names and the sections by their name and `IX`
//! ([`Passthrough::children_followed_by`] does the arithmetic, counting its keys once each).
//!
//! **What the model cannot see.** A cell put in a list of cells by `cells.insert`, and a row, a section or a shape put in or taken out by the methods of the list itself (`rows.push`,
//! `sections.retain`, `shapes.remove`), leave the count as it was: a writer that holds an element with nodes goes through the writers above. The engine's `places_guard` test holds
//! every place where one does not, in the engine and in the crates below it, to a list with the reason for each.

use std::sync::Arc;

use crate::DocError;
use crate::cell::Cell;
use crate::document::Document;
use crate::name::CellName;
use crate::package::Ext;
use crate::part::{Master, Page, Style};
use crate::raw::Passthrough;
use crate::shape::{Shape, ShapeList};
use crate::sheet::{CellAddr, Cells, Row, RowKey, Rows, Section, SectionKey, Sheet};
use crate::sheet_ref::{Owner, SheetRef};

/// A sheet to change, with the unknown nodes that count its cells and sections along with it.
pub struct Counted<'a> {
    pub sheet: &'a mut Sheet,
    /// The unknown nodes of the shape or the style that holds the sheet's cells and sections among its own children: they count those children too. None for a page's, a master's and
    /// the document's sheet, whose nodes are the sheet's own.
    pub owner: Option<&'a mut Passthrough>,
}

impl Document {
    /// The sheet `at` names, to change, with the nodes that count its children. Every `Arc` on the way is made unique first, so a copy of the document that shares them keeps what it
    /// had (`Arc::make_mut`). `None` for a sheet that is not there, and then nothing was copied.
    pub fn counted_mut(&mut self, at: &SheetRef) -> Option<Counted<'_>> {
        self.sheet_of(at)?;
        match at {
            SheetRef::Document => Some(Counted { sheet: Arc::make_mut(&mut self.sheet), owner: None }),
            SheetRef::Style(id) => {
                let found = self.styles.iter().position(|s| s.id == *id)?;
                Some(Arc::make_mut(self.styles.get_mut(found)?).counted_sheet())
            }
            SheetRef::Page(id) => {
                let found = self.pages.iter().position(|p| p.id == *id)?;
                let page = Arc::make_mut(self.pages.get_mut(found)?);
                Some(Counted { sheet: Arc::make_mut(&mut page.sheet), owner: None })
            }
            SheetRef::Master(id) => {
                let found = self.masters.iter().position(|m| m.id == *id)?;
                let master = Arc::make_mut(self.masters.get_mut(found)?);
                Some(Counted { sheet: Arc::make_mut(&mut master.sheet), owner: None })
            }
            SheetRef::Shape(at) => {
                let list = match at.owner {
                    Owner::Page(id) => {
                        let found = self.pages.iter().position(|p| p.id == id)?;
                        &mut Arc::make_mut(self.pages.get_mut(found)?).shapes
                    }
                    Owner::Master(id) => {
                        let found = self.masters.iter().position(|m| m.id == id)?;
                        &mut Arc::make_mut(self.masters.get_mut(found)?).shapes
                    }
                };
                let (first, rest) = at.path.split_first()?;
                let mut shape = Arc::make_mut(list.get_mut(usize::try_from(*first).ok()?)?);
                for step in rest {
                    shape = Arc::make_mut(shape.children.get_mut(usize::try_from(*step).ok()?)?);
                }
                Some(shape.counted_sheet())
            }
        }
    }
}

impl Shape {
    /// The shape's sheet, to change, with the shape's own nodes, which count the sheet's cells and sections too (see [`Counted`]). The sheet is made unique first.
    pub fn counted_sheet(&mut self) -> Counted<'_> {
        Counted { sheet: Arc::make_mut(&mut self.sheet), owner: Some(&mut self.passthrough) }
    }
}

impl Style {
    /// The style's sheet, to change, with the style's own nodes, which count the sheet's cells and sections too (see [`Counted`]). The sheet is made unique first.
    pub fn counted_sheet(&mut self) -> Counted<'_> {
        Counted { sheet: Arc::make_mut(&mut self.sheet), owner: Some(&mut self.passthrough) }
    }
}

impl Counted<'_> {
    /// Whether the nodes of the element that holds the sheet are any.
    fn owner_holds(&self) -> bool {
        self.owner.as_deref().is_some_and(|owner| !owner.is_empty())
    }

    /// The owner's nodes after the sheet made a section (it is the one at the end): the sections were `before` and the cells `offset`.
    fn section_made(&mut self, offset: usize, before: usize) {
        if self.sheet.sections.len() > before
            && let Some(owner) = self.owner.as_deref_mut()
        {
            owner.children_inserted(offset, before, &[before], false);
        }
    }

    /// The section with this key, to change, made after the others when the sheet has none (what stood after the last section stands after it still). The sheet's own nodes, and the
    /// owner's, are moved with it.
    pub fn section_mut(&mut self, key: &SectionKey) -> Result<&mut Section, DocError> {
        let (offset, before) = (self.sheet.cells.len(), self.sheet.sections.len());
        let slot = self.sheet.section_slot(key);
        self.section_made(offset, before);
        let at = slot?;
        self.sheet.sections.get_mut(at).map(Arc::make_mut).ok_or(DocError::NoSuchSheet)
    }

    /// The row with this key of this section, to change, making the section and the row if the sheet has neither: the nodes that count them move.
    pub fn row_mut(&mut self, key: &SectionKey, row: &RowKey) -> Result<&mut Row, DocError> {
        let (offset, before) = (self.sheet.cells.len(), self.sheet.sections.len());
        let slot = self.sheet.row_slot(key, row);
        self.section_made(offset, before);
        self.sheet.row_at_mut(&slot?)
    }

    /// Writes `cell` at `addr`, as [`Sheet::set_cell`] does, and moves the nodes that count what it made: a section, a row, or a cell.
    pub fn set_cell(&mut self, addr: &CellAddr, cell: Cell) -> Result<Option<Cell>, DocError> {
        if !self.owner_holds() {
            return self.sheet.set_cell(addr, cell);
        }
        let (cells, sections) = (self.sheet.cells.len(), self.sheet.sections.len());
        let rank = (addr.section.is_none() && !self.sheet.cells.contains_key(addr.cell.as_str())).then(|| self.sheet.rank_of(&addr.cell));
        let done = self.sheet.set_cell(addr, cell);
        let offset = self.sheet.cells.len();
        if let Some(rank) = rank.filter(|_| offset > cells)
            && let Some(owner) = self.owner.as_deref_mut()
        {
            owner.children_inserted(0, cells, &[rank], true);
        }
        self.section_made(offset, sections);
        done
    }

    /// Takes out the cell at `addr`, as [`Sheet::remove_cell`] does, and moves the nodes that counted it.
    pub fn remove_cell(&mut self, addr: &CellAddr) -> Option<Cell> {
        let place = (self.owner_holds() && addr.section.is_none()).then(|| (self.sheet.rank_of(&addr.cell), self.sheet.cells.len()));
        let gone = self.sheet.remove_cell(addr);
        if let (Some((rank, old_len)), Some(_), Some(owner)) = (place, gone.as_ref(), self.owner.as_deref_mut()) {
            owner.children_removed(0, old_len, &[rank]);
        }
        gone
    }

    /// Puts `section` among the sheet's sections at `at` (at the end when that is past them), and moves the nodes. → where it went.
    pub fn insert_section(&mut self, at: usize, section: Arc<Section>) -> usize {
        let (offset, old_len) = (self.sheet.cells.len(), self.sheet.sections.len());
        let at = self.sheet.insert_section(at, section);
        if let Some(owner) = self.owner.as_deref_mut() {
            owner.children_inserted(offset, old_len, &[at], false);
        }
        at
    }

    /// Puts `section` after the others ([`Counted::insert_section`]).
    pub fn push_section(&mut self, section: Arc<Section>) -> usize {
        self.insert_section(usize::MAX, section)
    }

    /// Takes out every section `is_it` says, and moves the nodes that stood among the sheet's children with them.
    pub fn remove_sections(&mut self, is_it: impl Fn(&Section) -> bool) {
        let (offset, old_len) = (self.sheet.cells.len(), self.sheet.sections.len());
        let gone = self.sheet.remove_sections(is_it);
        if let Some(owner) = self.owner.as_deref_mut() {
            owner.children_removed(offset, old_len, &gone);
        }
    }

    /// Keeps the sections `keep` says (the ones it does not are taken out): [`Counted::remove_sections`].
    pub fn retain_sections(&mut self, keep: impl Fn(&Section) -> bool) {
        self.remove_sections(|section| !keep(section));
    }
}

/// Moves the nodes that count `cells` (they are the first children of the element: its cells come first) after the change from the cells whose names are `before` to `now`: the cells
/// that are not in `now` are taken out and those that are not in `before` put in, each in the place the order of the names gives it.
pub fn cells_changed(nodes: &mut Passthrough, before: &[CellName], now: &Cells) {
    if nodes.is_empty() {
        return;
    }
    let (gone, put) = cell_changes(before, now.keys());
    nodes.children_removed(0, before.len(), &gone);
    nodes.children_inserted(0, before.len().saturating_sub(gone.len()), &put, true);
}

/// The names of the cells, when the nodes that count them hold any (the names are what [`cells_changed`] reads before the cells change), and none when they hold none.
pub fn cell_names(nodes: &Passthrough, cells: &Cells) -> Vec<CellName> {
    if nodes.is_empty() { Vec::new() } else { cells.keys().cloned().collect() }
}

/// The places in `before` of the names that are not in `now`, and the places in `now` of the names that are not in `before`, both in rising order (the two lists are in order).
pub(crate) fn cell_changes<'n>(before: &[CellName], now: impl Iterator<Item = &'n CellName>) -> (Vec<usize>, Vec<usize>) {
    let (mut gone, mut put) = (Vec::new(), Vec::new());
    let mut now = now.enumerate().peekable();
    for (i, held) in before.iter().enumerate() {
        // The names of `now` that come before this one are new.
        while let Some((j, _)) = now.next_if(|(_, name)| *name < held) {
            put.push(j);
        }
        if now.next_if(|(_, name)| *name == held).is_none() {
            gone.push(i);
        }
    }
    put.extend(now.map(|(j, _)| j));
    (gone, put)
}

impl Row {
    /// Gives the row these cells in place of its own, and moves the nodes that stood among the row's cells with the cells that stay (a node goes on following the cell it followed).
    pub fn replace_cells(&mut self, cells: Cells) {
        let before = cell_names(&self.passthrough, &self.cells);
        self.cells = cells;
        cells_changed(&mut self.passthrough, &before, &self.cells);
    }
}

// ------------------------------------------------------------------------------------------------------------------------ elements made again

/// The keys of a section's rows: the key a row has in a section of this name (`None` for a row with neither `IX` nor `N`, which is told from no other).
fn row_keys(name: &str, rows: &Rows) -> Vec<Option<RowKey>> {
    rows.iter().map(|row| row.key(name)).collect()
}

/// The keys of a sheet's sections: a section is the one with this name and this `IX`. A section that `stands` does not say it is the same as the one of its key is a section with no key,
/// which is the same as none (it went, and what is put in its place is new).
fn section_keys(sections: &[Arc<Section>], stands: &dyn Fn(&Section) -> bool) -> Vec<Option<SectionKey>> {
    sections.iter().map(|section| stands(section).then(|| SectionKey { name: section.name.clone(), ix: section.ix })).collect()
}

/// Moves `nodes`, which count the cells and then the sections of the sheet `old`, to count `cells` and `sections` (a sheet made again from `old`): **a node goes on following the cell or
/// the section it followed**, a cell or a section that is new goes ahead of the nodes that stood after the one before it, and a node whose cell or section is gone follows the one before.
/// The cells are told apart by their names and the sections by their name and `IX`; a section of `old` that does not `stand` is gone, whatever stands in its place.
fn children_rebuilt(nodes: &mut Passthrough, old: &Sheet, cells: &Cells, sections: &[Arc<Section>], stands: &dyn Fn(&Section) -> bool) {
    if nodes.is_empty() {
        return;
    }
    let before = cell_names(nodes, &old.cells);
    cells_changed(nodes, &before, cells);
    nodes.children_followed_by(cells.len(), &section_keys(&old.sections, stands), &section_keys(sections, &|_| true));
}

/// The nodes of the shape or the style that holds a sheet (they count its cells and then its sections, and what comes after is the text and the members) once the sheet was made again from
/// `old` as `new` is: each stays after the child it followed ([`Sheet::take_places_of`] does the same for the nodes of the sheet itself). None for a shape that holds none.
pub fn owner_places_after(nodes: &Passthrough, old: &Sheet, new: &Sheet) -> Passthrough {
    owner_places_after_where(nodes, old, new, |_| true)
}

/// [`owner_places_after`] for a sheet made again with some of the sections of `old` **replaced**: a section of `old` that `stands` does not say is the same section stays is gone, and the
/// section of its name and `IX` in `new` is a new one (the sections a change of master takes from the new master).
pub fn owner_places_after_where(nodes: &Passthrough, old: &Sheet, new: &Sheet, stands: impl Fn(&Section) -> bool) -> Passthrough {
    let mut out = nodes.clone();
    children_rebuilt(&mut out, old, &new.cells, &new.sections, &stands);
    out
}

impl Sheet {
    /// This sheet was made again from `old`, with other cells and other sections: it **takes the unknown nodes of `old` and moves them** so that each stays after the cell or the section it
    /// followed (the rule of [`Section::insert_row`] and the rest: a new child goes ahead of the nodes that stood after the child before it, and a node whose child is gone goes with the one
    /// before). The nodes of the shape or the style that holds the sheet are [`owner_places_after`]. A sheet that held no node costs nothing.
    pub fn take_places_of(&mut self, old: &Sheet) {
        self.take_places_of_where(old, |_| true);
    }

    /// [`Sheet::take_places_of`] where some sections of `old` are **replaced**: one that `stands` does not say is the same section stays is gone, and a section of its name and `IX` that this
    /// sheet holds is a new one ([`owner_places_after_where`]).
    pub fn take_places_of_where(&mut self, old: &Sheet, stands: impl Fn(&Section) -> bool) {
        self.passthrough = old.passthrough.clone();
        children_rebuilt(&mut self.passthrough, old, &self.cells, &self.sections, &stands);
    }
}

impl Section {
    /// Puts `rows` in place of the section's own (a route made again, a path carried on), and moves the nodes that stood among the section's children: **a node goes on following the row it
    /// followed, the row being told by its key** (`IX` or `N`), a node whose row is gone goes with the row before it, a row that is new goes ahead of the nodes that stood after the row
    /// before it, and a node before the first row stays before it. The keys are read once each, and only when the section holds a node.
    pub fn replace_rows(&mut self, rows: Rows) {
        if !self.passthrough.is_empty() {
            let name = self.name.as_str();
            self.passthrough.children_followed_by(self.cells.len(), &row_keys(name, &self.rows), &row_keys(name, &rows));
        }
        self.rows = rows;
    }

    /// This section was made again from `old`, with other cells and other rows: it **takes the unknown nodes of `old` and moves them** so that each stays after the cell or the row it
    /// followed (the cells told by their names and the rows by their keys; [`Sheet::take_places_of`] has the rule). A section that held no node costs nothing.
    pub fn take_places_of(&mut self, old: &Section) {
        self.passthrough = old.passthrough.clone();
        if self.passthrough.is_empty() {
            return;
        }
        let before = cell_names(&self.passthrough, &old.cells);
        cells_changed(&mut self.passthrough, &before, &self.cells);
        let name = self.name.as_str();
        self.passthrough.children_followed_by(self.cells.len(), &row_keys(name, &old.rows), &row_keys(name, &self.rows));
    }
}

// ------------------------------------------------------------------------------------------------------------------------ lists of shapes

/// The nodes of the `Shapes` element a page, a master or a group keeps in its slot, when it holds any.
fn kept_shapes<T: Default + Clone + PartialEq>(ext: &mut Ext<T>) -> Option<&mut Passthrough> {
    let holds = ext.get().and_then(|data| data.vsdx.0.iter().find(|(key, _)| &**key == "Shapes")).is_some_and(|(_, kept)| !kept.kept.is_empty());
    if !holds {
        return None;
    }
    ext.make_mut().vsdx.0.iter_mut().find(|(key, _)| &**key == "Shapes").map(|(_, kept)| &mut kept.kept)
}

/// What `change` does to the list of shapes, with **the unknown nodes among the shapes moved with the shapes they followed**: a node goes on following the shape it
/// followed (wherever the shape goes), a node whose shape is taken out goes with the shape before it, a shape put in goes ahead of a node that stood after the shape before it, and a
/// node before the first shape stays before it. The shapes are told apart by their `ID`; the list is read before and after only when a node is held, which is nearly never.
fn change_shapes<R>(list: &mut ShapeList, nodes: Option<&mut Passthrough>, change: impl FnOnce(&mut ShapeList) -> R) -> R {
    let Some(nodes) = nodes else { return change(list) };
    let old: Vec<u32> = list.iter().map(|shape| shape.id).collect();
    let done = change(list);
    let new: Vec<u32> = list.iter().map(|shape| shape.id).collect();
    nodes.children_followed(0, &old, &new);
    done
}

macro_rules! shape_list_writers {
    ($owner:ident, $list:ident, $edit:ident, $push:ident, $insert:ident, $remove:ident) => {
        impl $owner {
            /// Changes the list of shapes through `change` (any change: shapes put in, taken out, moved, a list put in place of the list), and moves the unknown nodes among the shapes with
            /// the shapes they followed (see the module documentation). The shapes are told apart by their `ID` and the list is read before and after, once each, **only when a node is held**.
            pub fn $edit<R>(&mut self, change: impl FnOnce(&mut ShapeList) -> R) -> R {
                change_shapes(&mut self.$list, kept_shapes(&mut self.ext), change)
            }

            /// Puts `shape` after the others: it goes ahead of a node that trailed the shapes. **The cost is the nodes held, and never the shapes** (a paste of thousands of shapes is a push
            /// for each).
            pub fn $push(&mut self, shape: Arc<Shape>) {
                let old_len = self.$list.len();
                self.$list.push(shape);
                if let Some(nodes) = kept_shapes(&mut self.ext) {
                    nodes.children_inserted(0, old_len, &[old_len], true);
                }
            }

            /// Puts `shape` at `at`: it goes ahead of a node that stood after the shape before it, and behind one that stood before the first. Refused (`Err`) for a place past the end.
            pub fn $insert(&mut self, at: usize, shape: Arc<Shape>) -> Result<(), crate::OutOfRange> {
                let old_len = self.$list.len();
                self.$list.insert(at, shape)?;
                if let Some(nodes) = kept_shapes(&mut self.ext) {
                    nodes.children_inserted(0, old_len, &[at], true);
                }
                Ok(())
            }

            /// Takes out the shape at `at`: the nodes that stood after it go with the shape before it.
            pub fn $remove(&mut self, at: usize) -> Option<Arc<Shape>> {
                let old_len = self.$list.len();
                let gone = self.$list.remove(at)?;
                if let Some(nodes) = kept_shapes(&mut self.ext) {
                    nodes.children_removed(0, old_len, &[at]);
                }
                Some(gone)
            }
        }
    };
}

shape_list_writers!(Page, shapes, edit_shapes, push_shape, insert_shape, remove_shape);
shape_list_writers!(Master, shapes, edit_shapes, push_shape, insert_shape, remove_shape);
shape_list_writers!(Shape, children, edit_members, push_member, insert_member, remove_member);
