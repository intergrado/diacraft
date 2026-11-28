//! The keyboard of the sheet (design 5.9, of the window spec): moving around, starting an edit and the keys inside one.
//!
//! **Browsing** (the sheet has the focus and no edit is open): the arrows move one cell, up and down in a column of pairs (a line that has no cell in the column is passed over, and the
//! column's end is the next section's header) and left and right along a line; Tab and Shift+Tab go on in reading order, on to the next line; Home and End are the ends of the line,
//! Ctrl+Home and Ctrl+End the corners of the sheet; Page Up and Page Down scroll a screen and the selection with it, and Ctrl with them a screen sideways; Enter goes down (a
//! spreadsheet's way: ours, R44 leaves it `unknown`); **F2** edits the formula's text, a **printable key** starts an edit with its character, **Delete and Backspace** with none (Enter
//! then clears the cell); Ctrl+C copies the formula and Ctrl+V commits the clipboard's text as one; **Space or Enter on a header** folds the section or opens it; **Shift+F10** or the
//! F10 opens the cell's right-click menu; **Ctrl+E opens the Edit Formula card**; **Escape gives the focus back to the drawing**.
//!
//! **Inside an edit**: Enter commits and goes down, Tab commits and goes right, Escape cancels, and in typing mode the arrows commit and move where in F2 mode they move the caret.
//!
//! While the sheet has the focus its keys are its own, so Delete does not delete the shape selected in the drawing, the arrows do not nudge it and Ctrl+C does not copy it
//! (`DiacraftApp::sheet_keys_held`); Ctrl+Z and Ctrl+Y are the drawing's undo and redo, whatever made the step. Moving is by **visual lines** (`pairs.rs`) and by what names a cell, so a
//! fold or a change of the list's width keeps the selection where it was.

use diacraft_engine::doc::Document;
use egui::os::OperatingSystem;
use egui::{Context, Event, InputState, Key, Modifiers};
use serde_json::{Value, json};

use super::edit::{Begin, MAX_EDIT_BYTES, Then, edit_text_of};
use super::place_params;
use super::scroll::Scroll;
use super::view::{Sel, Stop, View};
use crate::app::DiacraftApp;
use crate::shortcuts::{consume_exact, types_text};

/// The most lines a move looks through to find a place to stop at: a table's line of titles and a note have none, and a block of pairs is a few lines.
const MAX_SCAN: u64 = 8_192;

/// A direction to move in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Then {
    fn dir(self) -> Option<Dir> {
        match self {
            Then::Stay => None,
            Then::Down => Some(Dir::Down),
            Then::Up => Some(Dir::Up),
            Then::Left => Some(Dir::Left),
            Then::Right => Some(Dir::Right),
        }
    }
}

/// A selection as a row: a cell of a table's row is its row.
fn as_row(sel: Sel) -> Sel {
    match sel {
        Sel::Cell { section, row: Some(row), .. } => Sel::Row { section, row },
        other => other,
    }
}

/// The place after `from` one line up or down: in a block of pairs the same column (a line without it is passed over); elsewhere the nearest.
fn vertical(view: &mut View, doc: &Document, from: &Sel, down: bool) -> Option<Sel> {
    let n = view.vline_of(from)?;
    let cur = view.stop_of(from)?;
    let total = view.visual().total;
    let from_row = matches!(from, Sel::Row { .. });
    let mut m = n;
    for _ in 0..MAX_SCAN {
        m = if down { m.checked_add(1).filter(|m| *m < total)? } else { m.checked_sub(1)? };
        let stops = view.stops(doc, m);
        let Some(first) = stops.first() else { continue };
        let pick = if cur.strict && first.strict {
            stops.iter().find(|s| s.col == cur.col)
        } else {
            stops.iter().min_by_key(|s| (s.col.abs_diff(cur.col), s.col))
        };
        if let Some(stop) = pick {
            return Some(if from_row { as_row(stop.sel.clone()) } else { stop.sel.clone() });
        }
    }
    None
}

/// The place after `from` one cell left or right in its line. From a row's first cell, Left is the row; from the row, Right is its first cell.
fn horizontal(view: &mut View, doc: &Document, from: &Sel, right: bool) -> Option<Sel> {
    let n = view.vline_of(from)?;
    let stops = view.stops(doc, n);
    match from {
        Sel::Header { .. } => None,
        Sel::Row { .. } => right.then(|| stops.first().map(|s| s.sel.clone())).flatten(),
        Sel::Cell { section, row, .. } => {
            let at = stops.iter().position(|s| &s.sel == from)?;
            if right {
                stops.get(at + 1).map(|s| s.sel.clone())
            } else if at > 0 {
                stops.get(at - 1).map(|s| s.sel.clone())
            } else {
                row.clone().map(|row| Sel::Row { section: section.clone(), row })
            }
        }
    }
}

/// The cells of a line, a header left out.
fn cells_of(stops: Vec<Stop>) -> Vec<Stop> {
    stops.into_iter().filter(|s| !matches!(s.sel, Sel::Header { .. })).collect()
}

/// The next or the previous cell in reading order: along the line, then the first or the last cell of the next or the previous line that has one (a header is passed over).
fn tab_step(view: &mut View, doc: &Document, from: &Sel, forward: bool) -> Option<Sel> {
    let n = view.vline_of(from)?;
    let stops = view.stops(doc, n);
    let within = match from {
        Sel::Cell { .. } => stops
            .iter()
            .position(|s| &s.sel == from)
            .and_then(|at| if forward { stops.get(at + 1) } else { at.checked_sub(1).and_then(|p| stops.get(p)) }),
        Sel::Row { .. } if forward => stops.first(),
        _ => None,
    };
    if let Some(stop) = within {
        return Some(stop.sel.clone());
    }
    let total = view.visual().total;
    let mut m = n;
    for _ in 0..MAX_SCAN {
        m = if forward { m.checked_add(1).filter(|m| *m < total)? } else { m.checked_sub(1)? };
        let cells = cells_of(view.stops(doc, m));
        let pick = if forward { cells.first() } else { cells.last() };
        if let Some(stop) = pick {
            return Some(stop.sel.clone());
        }
    }
    None
}

/// The first or the last cell of the line `from` is on (a row selected goes to its last cell for End and stays for Home).
fn line_end(view: &mut View, doc: &Document, from: &Sel, last: bool) -> Option<Sel> {
    let n = view.vline_of(from)?;
    let stops = view.stops(doc, n);
    match from {
        Sel::Header { .. } => None,
        Sel::Row { .. } => last.then(|| stops.last().map(|s| s.sel.clone())).flatten(),
        Sel::Cell { .. } => if last { stops.last() } else { stops.first() }.map(|s| s.sel.clone()),
    }
}

/// The first cell of the sheet, or the last: the corners (Ctrl+End is the lower-right corner, Ctrl+Home read as the upper-left).
fn corner(view: &mut View, doc: &Document, last: bool) -> Option<Sel> {
    let total = view.visual().total;
    for k in 0..total.min(MAX_SCAN) {
        let m = if last { total - 1 - k } else { k };
        let cells = cells_of(view.stops(doc, m));
        if let Some(stop) = if last { cells.last() } else { cells.first() } {
            return Some(stop.sel.clone());
        }
    }
    None
}

/// The place `lines` visual lines down or up from `from`, at the nearest column; a line with no place is passed toward where it came from.
fn lines_away(view: &mut View, doc: &Document, from: &Sel, lines: u64, down: bool) -> Option<Sel> {
    let n = view.vline_of(from)?;
    let cur = view.stop_of(from)?;
    let total = view.visual().total;
    let target = if down { n.saturating_add(lines).min(total.saturating_sub(1)) } else { n.saturating_sub(lines) };
    let mut m = target;
    for _ in 0..MAX_SCAN {
        let stops = view.stops(doc, m);
        if let Some(stop) = stops.iter().min_by_key(|s| (s.col.abs_diff(cur.col), s.col)) {
            return Some(if matches!(from, Sel::Row { .. }) { as_row(stop.sel.clone()) } else { stop.sel.clone() });
        }
        m = if down { m.checked_sub(1)? } else { m.checked_add(1).filter(|m| *m < total)? };
        if m == n {
            break;
        }
    }
    None
}

/// The keys taken in one frame while no edit is open.
#[derive(Default)]
struct Nav {
    dirs: Vec<Dir>,
    tab: Option<bool>,
    home: bool,
    end: bool,
    ctrl_home: bool,
    ctrl_end: bool,
    page: Option<bool>,
    ctrl_page: Option<bool>,
    enter: bool,
    f2: bool,
    delete: bool,
    space: bool,
    escape: bool,
    menu: bool,
    formula: bool,
    typed: Option<String>,
    copy: bool,
    paste: Option<String>,
}

/// Takes the keys of the sheet from this frame's input: each is removed from it, so that the drawing's keys and egui's own never see it.
fn take_nav(i: &mut InputState, os: OperatingSystem, header: bool) -> Nav {
    let mut nav = Nav::default();
    for (key, dir) in [(Key::ArrowUp, Dir::Up), (Key::ArrowDown, Dir::Down), (Key::ArrowLeft, Dir::Left), (Key::ArrowRight, Dir::Right)] {
        if i.consume_key(Modifiers::SHIFT, key) || i.consume_key(Modifiers::NONE, key) {
            nav.dirs.push(dir);
        }
    }
    nav.tab = if i.consume_key(Modifiers::SHIFT, Key::Tab) {
        Some(false)
    } else if i.consume_key(Modifiers::NONE, Key::Tab) {
        Some(true)
    } else {
        None
    };
    // The chords with Ctrl are matched exactly: Ctrl and Alt together can be AltGr (`shortcuts::consume_exact`).
    nav.ctrl_home = consume_exact(i, Modifiers::COMMAND, Key::Home);
    nav.ctrl_end = consume_exact(i, Modifiers::COMMAND, Key::End);
    nav.home = i.consume_key(Modifiers::NONE, Key::Home);
    nav.end = i.consume_key(Modifiers::NONE, Key::End);
    nav.ctrl_page = if consume_exact(i, Modifiers::COMMAND, Key::PageDown) {
        Some(true)
    } else if consume_exact(i, Modifiers::COMMAND, Key::PageUp) {
        Some(false)
    } else {
        None
    };
    nav.page = if i.consume_key(Modifiers::NONE, Key::PageDown) {
        Some(true)
    } else if i.consume_key(Modifiers::NONE, Key::PageUp) {
        Some(false)
    } else {
        None
    };
    nav.enter = i.consume_key(Modifiers::SHIFT, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Enter);
    nav.f2 = i.consume_key(Modifiers::NONE, Key::F2);
    nav.delete = i.consume_key(Modifiers::NONE, Key::Delete) | i.consume_key(Modifiers::NONE, Key::Backspace);
    nav.escape = i.consume_key(Modifiers::NONE, Key::Escape);
    nav.menu = i.consume_key(Modifiers::SHIFT, Key::F10);
    nav.formula = i.consume_key(Modifiers::COMMAND, Key::E);
    if header {
        // Space folds a header; the text event that comes with the key is no character to type.
        nav.space = i.consume_key(Modifiers::NONE, Key::Space);
        if nav.space {
            i.events.retain(|e| !matches!(e, Event::Text(t) if t == " "));
        }
    }
    // A character typed (AltGr's and a Mac's Option's too), and not what a chord leaves behind.
    let plain = types_text(os, &i.modifiers);
    let mut typed = None;
    i.events.retain(|e| match e {
        Event::Copy => {
            nav.copy = true;
            false
        }
        // A cut changes the formula's cell, which a key does not do here: the key is taken and does nothing.
        Event::Cut => false,
        Event::Paste(text) => {
            nav.paste = Some(text.clone());
            false
        }
        Event::Text(text) if plain && typed.is_none() && !text.is_empty() && !text.chars().any(char::is_control) => {
            typed = Some(text.clone());
            false
        }
        _ => true,
    });
    nav.typed = typed;
    nav
}

/// The keys taken in one frame while an edit is open.
#[derive(Default)]
struct EditKeys {
    escape: bool,
    enter: Option<bool>,
    tab: Option<bool>,
    dir: Option<Dir>,
    f2: bool,
    formula: bool,
}

fn take_edit(i: &mut InputState, typing: bool) -> EditKeys {
    let mut keys = EditKeys { escape: i.consume_key(Modifiers::NONE, Key::Escape), ..EditKeys::default() };
    keys.enter = if i.consume_key(Modifiers::SHIFT, Key::Enter) {
        Some(true)
    } else if i.consume_key(Modifiers::NONE, Key::Enter) {
        Some(false)
    } else {
        None
    };
    keys.tab = if i.consume_key(Modifiers::SHIFT, Key::Tab) {
        Some(true)
    } else if i.consume_key(Modifiers::NONE, Key::Tab) {
        Some(false)
    } else {
        None
    };
    // The arrows are the editor's own in F2 mode (they move the caret) and move the selection in typing mode.
    if typing {
        for (key, dir) in [(Key::ArrowUp, Dir::Up), (Key::ArrowDown, Dir::Down), (Key::ArrowLeft, Dir::Left), (Key::ArrowRight, Dir::Right)] {
            if keys.dir.is_none() && (i.consume_key(Modifiers::SHIFT, key) || i.consume_key(Modifiers::NONE, key)) {
                keys.dir = Some(dir);
            }
        }
    }
    keys.f2 = i.consume_key(Modifiers::NONE, Key::F2);
    keys.formula = i.consume_key(Modifiers::COMMAND, Key::E);
    keys
}

impl DiacraftApp {
    /// Whether the sheet's keys are the ones in force: a sheet has the focus (the drawing's keys then stand aside). A text field of another part of the window that has the keyboard, a
    /// card or a menu over the window take it from the sheet; that is checked where the keys are run.
    pub(crate) fn sheet_keys_held(&self) -> bool {
        let sheet = &self.ui.sheet;
        sheet.focus && sheet.tabs.get(sheet.active).is_some_and(|t| t.view.as_ref().is_some_and(|v| !v.is_gone()))
    }

    /// Moves the selection of the tab on view one cell in the direction `then` says.
    pub(crate) fn sheet_move_selection(&mut self, then: Then) {
        let Some(dir) = then.dir() else { return };
        self.sheet_navigate(|view, doc, sel| match sel {
            Some(sel) => match dir {
                Dir::Up | Dir::Down => vertical(view, doc, sel, dir == Dir::Down),
                Dir::Left | Dir::Right => horizontal(view, doc, sel, dir == Dir::Right),
            },
            None => corner(view, doc, false),
        });
    }

    /// Sets the selection of the tab on view to what `find` says (given the view, the document and the selection there is), and brings it on view.
    fn sheet_navigate(&mut self, find: impl FnOnce(&mut View, &Document, Option<&Sel>) -> Option<Sel>) {
        let active = self.ui.sheet.active;
        let Some(doc) = self.session.document() else { return };
        let Some(tab) = self.ui.sheet.tabs.get_mut(active) else { return };
        let Some(view) = tab.view.as_mut() else { return };
        // A selection the sheet no longer shows (its section was folded or hidden) is its section's header, where a move begins.
        let current = tab.selected.clone().and_then(|sel| {
            if view.vline_of(&sel).is_some() {
                return Some(sel);
            }
            Some(Sel::Header { section: sel.section().to_owned() }).filter(|header| view.vline_of(header).is_some())
        });
        if let Some(next) = find(view, doc, current.as_ref()) {
            tab.selected = Some(next);
            tab.ensure_visible = true;
        }
    }

    /// Runs the sheet's keys for this frame, before the drawing's. With an edit open they are the editor's.
    pub(crate) fn run_sheet_keys(&mut self, ctx: &Context) {
        if !self.sheet_keys_held() || self.ui.backstage.is_some() || self.is_modal_open() {
            return;
        }
        let editing = self.sheet_editing();
        // Another text field has the keyboard (a pane's, the Font list's): its keys are its own.
        let other_field = ctx.text_edit_focused() && ctx.memory(|m| m.focused()) != Some(super::edit::edit_id());
        if other_field && !editing {
            return;
        }
        // The sheet's own menu: Escape closes it and every other key waits.
        if self.ui.sheet.menu.is_some() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.ui.sheet.menu = None;
            }
            return;
        }
        // A menu or card of the ribbon over the window has the keys.
        if egui::Popup::is_any_open(ctx) {
            return;
        }
        if editing {
            self.sheet_edit_keys(ctx);
        } else {
            self.sheet_nav_keys(ctx);
        }
    }

    /// The keys of an open edit.
    fn sheet_edit_keys(&mut self, ctx: &Context) {
        let typing = self.ui.sheet.tabs.get(self.ui.sheet.active).and_then(|t| t.edit.as_ref()).is_some_and(|e| e.typing);
        let keys = ctx.input_mut(|i| take_edit(i, typing));
        if keys.f2
            && let Some(e) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active).and_then(|t| t.edit.as_mut())
        {
            e.typing = false;
        }
        let taken = keys.escape || keys.enter.is_some() || keys.tab.is_some() || keys.dir.is_some() || keys.f2 || keys.formula;
        if keys.formula {
            self.sheet_formula_key();
        }
        if keys.escape {
            self.sheet_cancel_edit();
        } else if let Some(shift) = keys.enter {
            self.sheet_commit_edit(if shift { Then::Up } else { Then::Down });
        } else if let Some(shift) = keys.tab {
            self.sheet_commit_edit(if shift { Then::Left } else { Then::Right });
        } else if let Some(dir) = keys.dir {
            self.sheet_commit_edit(match dir {
                Dir::Up => Then::Up,
                Dir::Down => Then::Down,
                Dir::Left => Then::Left,
                Dir::Right => Then::Right,
            });
        }
        // A key taken changed what the next frame draws; a frame with none asks for nothing (no repaint loop).
        if taken {
            ctx.request_repaint();
        }
    }

    /// The keys that browse the sheet.
    fn sheet_nav_keys(&mut self, ctx: &Context) {
        let active = self.ui.sheet.active;
        let selected = self.ui.sheet.tabs.get(active).and_then(|t| t.selected.clone());
        let header = matches!(selected, Some(Sel::Header { .. }));
        // Before the input is borrowed: `Context::os` reads the context, which `input_mut` holds.
        let os = ctx.os();
        let nav = ctx.input_mut(|i| take_nav(i, os, header));
        let mut moved = false;
        for dir in &nav.dirs {
            self.sheet_move_selection(match dir {
                Dir::Up => Then::Up,
                Dir::Down => Then::Down,
                Dir::Left => Then::Left,
                Dir::Right => Then::Right,
            });
            moved = true;
        }
        if let Some(forward) = nav.tab {
            self.sheet_navigate(|view, doc, sel| match sel {
                Some(s) => tab_step(view, doc, s, forward),
                None => corner(view, doc, false),
            });
            moved = true;
        }
        if nav.home || nav.end {
            let last = nav.end;
            self.sheet_navigate(|view, doc, sel| sel.and_then(|s| line_end(view, doc, s, last)));
            moved = true;
        }
        if nav.ctrl_home || nav.ctrl_end {
            let last = nav.ctrl_end;
            self.sheet_navigate(|view, doc, _| corner(view, doc, last));
            moved = true;
        }
        if let Some(down) = nav.page {
            self.sheet_page(down);
            moved = true;
        }
        if let Some(right) = nav.ctrl_page
            && let Some(tab) = self.ui.sheet.tabs.get_mut(active)
        {
            let by = tab.page_w.max(1.0);
            tab.hscroll = (tab.hscroll + if right { by } else { -by }).max(0.0);
            moved = true;
        }
        if nav.enter || nav.space {
            if let Some(Sel::Header { section }) = &selected {
                // A header folds or opens (the command a press on it runs).
                if let Err(e) = self.execute("window.sheetCollapse", &json!({"section": section})) {
                    self.set_notice(&e.to_string());
                }
            } else {
                self.sheet_move_selection(Then::Down);
            }
            moved = true;
        }
        if nav.menu {
            let at = self.sheet_selected_at().unwrap_or_default();
            self.ui.sheet.menu = Some(at);
        }
        if nav.formula {
            self.sheet_formula_key();
        }
        if nav.f2 {
            self.sheet_try_begin(Begin::Text { in_bar: false });
        }
        if nav.delete {
            self.sheet_try_begin(Begin::Typing(String::new()));
        }
        if let Some(text) = nav.typed.filter(|_| !header) {
            self.sheet_try_begin(Begin::Typing(text));
        }
        if nav.copy {
            self.sheet_copy_formula(ctx);
        }
        if let Some(text) = nav.paste {
            self.sheet_paste_formula(text);
        }
        if nav.escape {
            self.ui.sheet.focus = false;
        }
        if moved || nav.f2 || nav.delete || nav.menu || nav.escape || nav.formula {
            ctx.request_repaint();
        }
    }

    /// Scrolls a screen down or up and moves the selection with it, as far as the sheet goes.
    fn sheet_page(&mut self, down: bool) {
        let active = self.ui.sheet.active;
        let page = self.ui.sheet.tabs.get(active).map_or(1, |t| t.page_lines.max(1));
        self.sheet_navigate(|view, doc, sel| match sel {
            Some(sel) => lines_away(view, doc, sel, page, down),
            None => corner(view, doc, false),
        });
        if let Some(tab) = self.ui.sheet.tabs.get_mut(active) {
            let total = tab.view.as_ref().map_or(0, |v| v.visual().total);
            let max_first = total.saturating_sub(page);
            let first = tab.scroll.first;
            let next = if down { first.saturating_add(page) } else { first.saturating_sub(page) };
            tab.scroll = Scroll::at(next).clamped(max_first);
        }
    }

    /// Begins an edit as a key asks for it, and says in the footer why it cannot.
    pub(crate) fn sheet_try_begin(&mut self, how: Begin) {
        let selected_cell = self.ui.sheet.tabs.get(self.ui.sheet.active).is_some_and(|t| matches!(t.selected, Some(Sel::Cell { .. })));
        if !selected_cell {
            return;
        }
        if let Err(words) = self.sheet_begin_edit(how) {
            self.sheet_say(words);
        }
    }

    /// Puts `words` in the footer for as long as the selection stays where it is.
    pub(crate) fn sheet_say(&mut self, words: String) {
        if let Some(tab) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active) {
            tab.note = Some((tab.selected.clone(), words));
        }
    }

    /// The whole formula of the selected cell as an edit holds it (`sheet.cell`, a query), or `None` when no cell is selected.
    fn sheet_selected_formula(&mut self) -> Option<String> {
        let active = self.ui.sheet.active;
        let doc = self.session.document()?;
        let tab = self.ui.sheet.tabs.get_mut(active)?;
        let sel = tab.selected.clone()?;
        let (_, cell) = tab.view.as_mut()?.cell_of(doc, &sel)?;
        let reference = cell.reference?;
        let mut params = place_params(&tab.place);
        if let Some(map) = params.as_object_mut() {
            map.insert("cell".to_owned(), json!(reference));
        }
        let answer = self.session.execute("sheet.cell", &params).ok()?;
        answer.get("formula").and_then(Value::as_str).map(edit_text_of)
    }

    /// Ctrl+C: the selected cell's formula goes to the system clipboard (the drawing's shapes are not copied while the sheet has the focus).
    fn sheet_copy_formula(&mut self, ctx: &Context) {
        if let Some(text) = self.sheet_selected_formula().filter(|t| !t.is_empty()) {
            ctx.copy_text(text);
        }
    }

    /// Ctrl+V on a selected cell: the clipboard's text is committed as its formula, one step (the same path as a typed text, so that a refusal keeps the text in the editor with the
    /// command's words).
    fn sheet_paste_formula(&mut self, text: String) {
        let selected_cell = self.ui.sheet.tabs.get(self.ui.sheet.active).is_some_and(|t| matches!(t.selected, Some(Sel::Cell { .. })));
        if !selected_cell {
            return;
        }
        if text.len() > MAX_EDIT_BYTES {
            self.sheet_say("The text on the clipboard is longer than 65,536 characters: a formula is at most that long.".to_owned());
            return;
        }
        match self.sheet_begin_edit(Begin::Typing(text)) {
            Ok(()) => {
                self.sheet_commit_edit(Then::Stay);
            }
            Err(words) => self.sheet_say(words),
        }
    }

    /// Where the selected cell was drawn, for a menu opened by a key: its lower left corner.
    fn sheet_selected_at(&self) -> Option<egui::Pos2> {
        let tab = self.ui.sheet.tabs.get(self.ui.sheet.active)?;
        tab.sel_rect.map(|rect| rect.left_bottom())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_of_a_row_selected_as_a_row_is_its_row_and_nothing_else_changes() {
        let cell = Sel::Cell { section: "Prop".to_owned(), row: Some(diacraft_engine::doc::RowKey::name("Cost")), cell: "Value".to_owned() };
        assert_eq!(as_row(cell), Sel::Row { section: "Prop".to_owned(), row: diacraft_engine::doc::RowKey::name("Cost") });
        let group = Sel::Cell { section: "Shape Transform".to_owned(), row: None, cell: "Width".to_owned() };
        assert_eq!(as_row(group.clone()), group);
        let header = Sel::Header { section: "A".to_owned() };
        assert_eq!(as_row(header.clone()), header);
    }

    #[test]
    fn a_commit_moves_by_the_direction_it_names_and_stay_moves_nowhere() {
        assert_eq!(Then::Down.dir(), Some(Dir::Down));
        assert_eq!(Then::Up.dir(), Some(Dir::Up));
        assert_eq!(Then::Left.dir(), Some(Dir::Left));
        assert_eq!(Then::Right.dir(), Some(Dir::Right));
        assert_eq!(Then::Stay.dir(), None);
    }
}
