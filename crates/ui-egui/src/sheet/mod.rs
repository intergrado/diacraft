//! The ShapeSheet window (SHEETWIN): the sections, rows and cells of a shape, a page, a master, a style or the document, each cell's formula and the value it works out to, side by side.
//! The design is the ShapeSheet window's (5 for the window, 6 for the budgets, 10 for the names and the tokens) and the spec of the window is
//! This is **, the sheet area, opening and reading**, and **, editing**.
//!
//! * [`SheetState`] is what the window remembers: the tabs (at most 32, each holding its object **by owner and `ID`, never by a path**), which one is on top, where the area docks, the
//!   two view toggles, and whether the sheet has the focus. None of it is the drawing and none is saved.
//! * `area.rs` docks the area **below** the drawing (45% of the middle band, 160 points at least, the drawing keeping 120), **beside** it, or **filling** the band; `tabs.rs` is its tab
//!   strip; `bar.rs` is the formula bar under the ribbon; `grid.rs` paints the lines on view; `scroll.rs` is the list's own scroll; `cell.rs` is a cell's box; `cards.rs` is Show Sections;
//!   `view.rs` is the cache of a tab (its outline and the cells of the lines last seen), `pairs.rs` lays a display group out as name and value pairs in columns, `edit.rs` is the
//!   cell editor and what a commit does, and `keys.rs` is the keyboard.
//! * The window reads through `diacraft_engine::sheetview`, on the session's document. **It edits only through the engine's `sheet.set_cell`**, one undo step each (a commit is the
//!   formula bar's accept, Enter, Tab, an arrow in typing mode, a press on another cell or the paste of a formula): it holds its selection, its folds and its editor itself, and shows
//!   the command's refusal in words and what `recalculated` says.
//! * The window commands are `window.shapesheet` (opens a sheet), `window.sheetTab`, `window.sheetClose`, `window.sheetDock`, `window.sheetView`, `window.sheetSections`,
//!   `window.sheetCollapse`, `window.sheetSelect` and `window.sheetEdit`: what a click does is what the command does.

mod area;
mod bar;
pub(crate) mod cards;
mod cell;
mod edit;
mod grid;
mod keys;
mod menu;
mod pairs;
mod rows;
mod scroll;
mod tabs;
mod view;

use std::collections::{HashMap, HashSet};

use diacraft_engine::doc::{Owner, RowKey, row_number_to_ix};
use diacraft_engine::sheetview::Place;
use diacraft_engine::{EngineError, Result, shown};
use egui::Rect;
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use bar::BarText;
use cell::Painted;
use edit::EditState;
use grid::ColDrag;
use pairs::pair_columns;
use scroll::Scroll;
use view::{Sel, View};

/// The id of the list's controls.
pub(crate) const LIST_ID: &str = "sheet_list";

/// The most tabs the sheet area holds: the 33rd Show reuses the tab used least recently (design 5.1).
pub const MAX_TABS: usize = 32;

/// Where the sheet area sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dock {
    /// Below the drawing (the default).
    Below,
    /// At the right of the drawing, left of the task panes.
    Beside,
    /// The whole middle band.
    Fill,
}

impl Dock {
    /// The word the commands and the window state call it.
    pub const fn word(self) -> &'static str {
        match self {
            Dock::Below => "below",
            Dock::Beside => "beside",
            Dock::Fill => "fill",
        }
    }

    pub fn from_word(word: &str) -> Option<Dock> {
        [Dock::Below, Dock::Beside, Dock::Fill].into_iter().find(|d| d.word().eq_ignore_ascii_case(word.trim()))
    }
}

/// What the bar was last worked out for: the drawing's generation and revision, the arrangement of the tab and the selection.
type BarKey = (u64, u64, u64, Option<Sel>);

/// One sheet of the area.
pub struct SheetTab {
    /// The object, by owner and `ID`.
    pub(crate) place: Place,
    /// The title the strip shows, made again when the drawing changes (`Place::title`).
    pub(crate) title: String,
    /// When the tab was last brought forward: the tab used least recently is the one a 33rd sheet takes the place of.
    pub(crate) last_used: u64,
    /// The sections folded to their headers, by name: the tab's own state, not saved.
    pub(crate) collapsed: HashSet<String>,
    /// The sections Show Sections hides, by name.
    pub(crate) hidden: HashSet<String>,
    /// Moves whenever a fold or a hidden section changes, so that the outline is arranged again.
    pub(crate) arrange_version: u64,
    pub(crate) selected: Option<Sel>,
    pub(crate) scroll: Scroll,
    pub(crate) hscroll: f32,
    /// The width the widest table on view wanted in the last frame: whether the horizontal bar is needed.
    pub(crate) content_w: f32,
    /// The selected cell is to be brought on view in the next frame.
    pub(crate) ensure_visible: bool,
    pub(crate) view: Option<View>,
    /// What the bar and the footer say of the selected cell, with what it was worked out for.
    pub(crate) bar: Option<(BarKey, BarText, String)>,
    /// The edit that is open on this tab: the tab's own, so that a tab brought forward keeps its text.
    pub(crate) edit: Option<EditState>,
    /// What the footer says (a commit's words, why a key could not begin an edit), for as long as the selection is the one it was said for.
    pub(crate) note: Option<(Option<Sel>, String)>,
    /// The widths a drag made columns, by section and title: the tab's own state, not saved.
    pub(crate) col_widths: HashMap<(String, String), f32>,
    pub(crate) col_drag: Option<ColDrag>,
    /// How many whole lines fit the list and how wide it is, as the last frame had them: a page key scrolls by these.
    pub(crate) page_lines: u64,
    pub(crate) page_w: f32,
    /// The width the lines are laid out in, as the last frame had it: how many columns of pairs fit.
    pub(crate) view_w: f32,
    /// What the selection is, for the controls of the Sections, Rows and Editing groups, with what it was made for.
    pub(crate) facts: Option<(BarKey, rows::Facts)>,
    /// Where the selected cell was drawn in the last frame.
    pub(crate) sel_rect: Option<Rect>,
}

impl SheetTab {
    fn new(place: Place, used: u64) -> SheetTab {
        SheetTab {
            place,
            title: String::new(),
            last_used: used,
            collapsed: HashSet::new(),
            hidden: HashSet::new(),
            arrange_version: 0,
            selected: None,
            scroll: Scroll::default(),
            hscroll: 0.0,
            content_w: 0.0,
            ensure_visible: false,
            view: None,
            bar: None,
            edit: None,
            note: None,
            col_widths: HashMap::new(),
            col_drag: None,
            page_lines: 10,
            page_w: 400.0,
            view_w: 0.0,
            facts: None,
            sel_rect: None,
        }
    }
}

/// The work the sheets did since the window began, as counts: a test holds a cost to these and never to a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Outlines made.
    pub outlines: u64,
    /// Rows those walks visited.
    pub rows: u64,
    /// Cells resolved to be shown.
    pub cells: u64,
    /// Lines read.
    pub lines: u64,
    /// Times `sheet.evaluate` was asked for the value of a text being typed.
    pub evaluations: u64,
}

/// A cell a commit worked out again, by the sheet it is on (`Place::json`) and its reference.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub sheet: Value,
    pub cell: Option<String>,
}

/// What the last commit of the window came to: the command's answer, or its refusal in words.
#[derive(Clone, Debug, PartialEq)]
pub enum Last {
    Done(Value),
    Refused(String),
}

impl Last {
    fn json(&self) -> Value {
        match self {
            Last::Done(answer) => json!({"ok": true, "answer": answer, "error": Value::Null}),
            Last::Refused(words) => json!({"ok": false, "answer": Value::Null, "error": words}),
        }
    }
}

/// What the window remembers of its ShapeSheet windows.
pub struct SheetState {
    pub(crate) tabs: Vec<SheetTab>,
    pub(crate) active: usize,
    pub(crate) dock: Dock,
    /// The dock the area had before Fill: where the drawing's own tab goes back to.
    pub(crate) back_to: Dock,
    /// The dock as it was drawn: Beside is shown as Below while the middle band is narrower than 640 points.
    pub(crate) shown_dock: Dock,
    /// The View group's two toggles: both on at first, and at least one stays on.
    pub(crate) formulas: bool,
    pub(crate) values: bool,
    /// The sheet has the focus: from a press in the area or its bar until a press in the drawing. The ShapeSheet Design tab shows while it does.
    pub(crate) focus: bool,
    clock: u64,
    /// Where the area and the bar were drawn in the last frame.
    pub(crate) area: Option<Rect>,
    pub(crate) bar: Option<Rect>,
    /// The boxes the last frame painted, for the tests and `ui.inspect`.
    pub(crate) painted: Vec<Painted>,
    /// The lines the last frame had on view.
    pub(crate) visible: (u64, u64),
    pub(crate) work: Totals,
    titles_for: Option<(u64, u64)>,
    /// The tab the strip last scrolled to.
    pub(crate) scrolled_to: Option<usize>,
    /// What the window's last commit came to.
    pub(crate) last: Option<Last>,
    /// The cells the last commit worked out again, marked until the next commit (no timer), and the revision they were marked at: another change of the
    /// drawing takes the marks away.
    pub(crate) changed: Vec<Changed>,
    pub(crate) changed_at: Option<u64>,
    /// The sheet's right-click menu, where it was opened.
    pub(crate) menu: Option<egui::Pos2>,
    /// The names of the rows the last frame drew, with their inks.
    pub(crate) row_names: Vec<(String, &'static str)>,
    /// The Insert Section card, while it is open (the Edit Formula card is the open edit of a tab, `in_card`).
    pub(crate) insert: Option<cards::InsertCard>,
}

impl Default for SheetState {
    fn default() -> SheetState {
        SheetState {
            tabs: Vec::new(),
            active: 0,
            dock: Dock::Below,
            back_to: Dock::Below,
            shown_dock: Dock::Below,
            formulas: true,
            values: true,
            focus: false,
            clock: 0,
            area: None,
            bar: None,
            painted: Vec::new(),
            visible: (0, 0),
            work: Totals::default(),
            titles_for: None,
            scrolled_to: None,
            last: None,
            changed: Vec::new(),
            changed_at: None,
            menu: None,
            row_names: Vec::new(),
            insert: None,
        }
    }
}

impl SheetState {
    /// Opens the sheet of `place`, or brings its tab forward: no two tabs show one sheet. With 32 tabs open the least recently used takes its place. The sheet has the focus.
    pub fn open(&mut self, place: Place) {
        self.clock += 1;
        self.focus = true;
        self.titles_for = None;
        if let Some(i) = self.tabs.iter().position(|t| t.place == place) {
            self.activate(i);
            return;
        }
        let tab = SheetTab::new(place, self.clock);
        if self.tabs.len() >= MAX_TABS {
            let victim = self.tabs.iter().enumerate().min_by_key(|(_, t)| t.last_used).map_or(0, |(i, _)| i);
            if let Some(slot) = self.tabs.get_mut(victim) {
                *slot = tab;
                self.active = victim;
            }
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
    }

    /// Brings the tab `i` on top.
    pub fn activate(&mut self, i: usize) {
        self.clock += 1;
        if let Some(tab) = self.tabs.get_mut(i) {
            tab.last_used = self.clock;
            self.active = i;
        }
    }

    /// Closes the tab `i`: the one on top is kept if it is another, and the area goes with the last.
    pub fn close(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.active = 0;
            self.focus = false;
            self.area = None;
            self.bar = None;
            self.painted.clear();
            self.menu = None;
            self.insert = None;
            return;
        }
        if i < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
    }

    /// A drawing replaced the open one: its sheets are not the new drawing's.
    pub fn forget_drawing(&mut self) {
        self.tabs.clear();
        self.active = 0;
        self.focus = false;
        self.area = None;
        self.bar = None;
        self.painted.clear();
        self.titles_for = None;
        self.scrolled_to = None;
        self.last = None;
        self.changed.clear();
        self.changed_at = None;
        self.menu = None;
        self.row_names.clear();
        self.insert = None;
    }
}

/// Whether the shortcut of the command `id` still runs while the sheet has the focus: files, the window's own commands and views, and undo and redo (the drawing's, whatever made
/// the step). The commands that change the drawing wait: the sheet's keys are its own.
pub(crate) fn shortcut_allowed(id: &str) -> bool {
    matches!(id, "edit.undo" | "edit.redo") || ["file.", "view.", "window.", "app.", "tool."].iter().any(|prefix| id.starts_with(prefix))
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

fn whole(cmd: &str, p: &Map<String, Value>, key: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number"))),
    }
}

fn word<'a>(cmd: &str, p: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

fn flag(cmd: &str, p: &Map<String, Value>, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The params that name `place` in the engine's `sheet.*` commands (design 7.1).
fn place_params(place: &Place) -> Value {
    match place {
        Place::Shape { owner: Owner::Page(page), id } => json!({"id": id, "page": page}),
        Place::Shape { owner: Owner::Master(master), id } => json!({"id": id, "master": master}),
        Place::Page(page) => json!({"page": page}),
        Place::Master(master) => json!({"master": master}),
        Place::Style(style) => json!({"style": style}),
        Place::Document => json!({"document": true}),
    }
}

/// What an object that is gone is called in the words of its tab.
fn gone_words(place: &Place) -> &'static str {
    match place {
        Place::Shape { .. } => "This shape is not in the drawing.",
        Place::Page(_) => "This page is not in the drawing.",
        Place::Master(_) => "This master is not in the drawing.",
        Place::Style(_) => "This style is not in the drawing.",
        Place::Document => "This drawing has no sheet.",
    }
}

impl DiacraftApp {
    // ---- reading ------------------------------------------------------------------------------------------------------------------------

    /// Makes ready what the sheet area and the bar draw from: the titles of the tabs (once for each revision of the drawing), the outline of the tab on view (once for each generation,
    /// revision and object), its arrangement, and what the bar says of the selected cell. A tab that is not on view reads nothing.
    pub(crate) fn sheet_prepare(&mut self) {
        let (generation, revision) = (self.ui.generation, self.session.revision());
        let Some(doc) = self.session.document() else { return };
        let sheet = &mut self.ui.sheet;
        if sheet.tabs.is_empty() {
            return;
        }
        if sheet.titles_for != Some((generation, revision)) {
            for tab in &mut sheet.tabs {
                tab.title = shown(&tab.place.title(doc), 120);
            }
            sheet.titles_for = Some((generation, revision));
        }
        let active = sheet.active.min(sheet.tabs.len() - 1);
        sheet.active = active;
        let mut made = (0, 0);
        let mut whole_wanted: Option<(Place, String)> = None;
        if let Some(tab) = sheet.tabs.get_mut(active) {
            if tab.view.as_ref().is_none_or(|v| v.generation != generation || v.revision != revision) {
                let view = View::build(doc, &tab.place, generation, revision);
                made = (1, view.rows_visited);
                tab.view = Some(view);
                tab.bar = None;
                tab.facts = None;
            }
            if let Some(view) = tab.view.as_mut() {
                tab.scroll.first = view.arrange(tab.arrange_version, &tab.hidden, &tab.collapsed, pair_columns(tab.view_w), tab.scroll.first);
                // The selected cell, for the bar and the footer: worked out again only when something it depends on has changed.
                let key: BarKey = (generation, revision, tab.arrange_version, tab.selected.clone());
                if tab.bar.as_ref().is_none_or(|(k, _, _)| *k != key) {
                    let (text, footer, truncated) = bar_of(view, doc, tab.selected.as_ref());
                    if truncated && let Some(Sel::Cell { .. }) = &tab.selected {
                        whole_wanted = text_ref(&text).map(|r| (tab.place.clone(), r));
                    }
                    tab.bar = Some((key.clone(), text, footer));
                }
                // What the selection is, for the controls of the Sections, Rows and Editing groups: made again only when the drawing or the selection changes.
                if tab.facts.as_ref().is_none_or(|(k, _)| *k != key) {
                    let facts = rows::facts_of(view, doc, tab.place.kind(), tab.selected.as_ref());
                    tab.facts = Some((key, facts));
                }
            }
        }
        sheet.work.outlines += made.0;
        sheet.work.rows += made.1;
        // The marks of what a commit worked out again go when the drawing changes by anything else (an undo, a move): they would mark cells that did not change.
        if sheet.changed_at.is_some_and(|at| at != revision) {
            sheet.changed.clear();
            sheet.changed_at = None;
        }
        // A formula cut at 4,096 characters in the reading is read whole for the bar (`sheet.cell`: a query, which changes nothing).
        if let Some((place, reference)) = whole_wanted {
            let mut params = place_params(&place);
            if let Some(map) = params.as_object_mut() {
                map.insert("cell".to_owned(), json!(reference));
            }
            if let Ok(answer) = self.session.execute("sheet.cell", &params)
                && let Some(formula) = answer.get("formula").and_then(Value::as_str)
                && let Some(tab) = self.ui.sheet.tabs.get_mut(active)
                && let Some((_, text, _)) = tab.bar.as_mut()
            {
                text.formula = formula.to_owned();
            }
        }
        // The value the text being typed works out to: asked for again only when the text has changed.
        self.sheet_edit_live();
    }

    /// Whether the ShapeSheet Design tab shows: a sheet is open and has the focus.
    pub fn sheet_tab_shown(&self) -> bool {
        !self.ui.sheet.tabs.is_empty() && self.ui.sheet.focus
    }

    /// Whether a press landed in the sheet area or its bar, or in the drawing: the sheet takes the focus from the first and gives it up to the second. A press in the ribbon or a
    /// menu changes nothing: the ShapeSheet Design tab's own controls are in the ribbon.
    pub(crate) fn sheet_focus_update(&mut self, ctx: &egui::Context) {
        if self.ui.sheet.tabs.is_empty() {
            return;
        }
        let (pressed, at) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos()));
        let Some(at) = at.filter(|_| pressed) else { return };
        // A press on a popup (a menu over the drawing) is the popup's, whatever lies under it.
        if ctx.layer_id_at(at).is_some_and(|layer| layer.order == egui::Order::Foreground) {
            return;
        }
        let sheet = &self.ui.sheet;
        let in_sheet = sheet.area.is_some_and(|r| r.contains(at)) || sheet.bar.is_some_and(|r| r.contains(at));
        let in_drawing = sheet.shown_dock != Dock::Fill && self.ui.canvas_rect.is_some_and(|r| r.contains(at));
        if in_sheet {
            self.ui.sheet.focus = true;
        } else if in_drawing {
            self.ui.sheet.focus = false;
        }
    }

    /// The drawing replaced the open one: its sheets go.
    pub(crate) fn forget_sheets(&mut self) {
        self.ui.sheet.forget_drawing();
    }

    /// The window state's account of the sheets (`ui.inspect`, the window commands' answers).
    pub(crate) fn sheet_state_json(&self) -> Value {
        let sheet = &self.ui.sheet;
        let tab = sheet.tabs.get(sheet.active);
        let view = tab.and_then(|t| t.view.as_ref());
        let sections: Vec<Value> = match (tab, view) {
            (Some(tab), Some(view)) => view
                .base()
                .sections
                .iter()
                .map(|s| json!({"name": s.name, "label": s.label, "open": !tab.collapsed.contains(&s.name), "shown": !tab.hidden.contains(&s.name)}))
                .collect(),
            _ => Vec::new(),
        };
        let selected = tab.and_then(|t| t.selected.as_ref()).map(|s| match s {
            Sel::Cell { section, row, cell } => json!({"section": section, "row": row.as_ref().map(row_json), "cell": cell, "header": false}),
            Sel::Row { section, row } => json!({"section": section, "row": row_json(row), "cell": Value::Null, "header": false}),
            Sel::Header { section } => json!({"section": section, "row": Value::Null, "cell": Value::Null, "header": true}),
        });
        let changed: Vec<String> = tab.map(|t| sheet.changed_on(&t.place.json()).into_iter().collect()).unwrap_or_default();
        let mut columns: Map<String, Value> = Map::new();
        if let Some(t) = tab {
            for ((section, title), width) in &t.col_widths {
                if let Value::Object(map) = columns.entry(section.clone()).or_insert_with(|| json!({})) {
                    map.insert(title.clone(), json!(width));
                }
            }
        }
        let bar = tab
            .and_then(|t| t.bar.as_ref())
            .map(|(_, b, _)| json!({"reference": b.reference, "formula": b.formula, "value": b.value, "error": b.error}));
        let painted: Vec<Value> = sheet
            .painted
            .iter()
            .map(|c| {
                json!({
                    "name": c.name, "ref": c.reference, "origin": match c.origin {
                        diacraft_engine::sheetview::Origin::Local => "local",
                        diacraft_engine::sheetview::Origin::Inherited => "inherited",
                        diacraft_engine::sheetview::Origin::Default => "default",
                    },
                    "ink": c.ink.word(), "valueInk": c.value_ink.word(), "dot": c.dot, "text": c.text, "shown": c.shown, "changed": c.changed,
                })
            })
            .collect();
        json!({
            "open": !sheet.tabs.is_empty(),
            "focus": sheet.focus,
            "dock": sheet.dock.word(),
            "shownDock": sheet.shown_dock.word(),
            "formulas": sheet.formulas,
            "values": sheet.values,
            "active": if sheet.tabs.is_empty() { Value::Null } else { json!(sheet.active) },
            "tabs": sheet.tabs.iter().map(|t| json!({"title": t.title, "sheet": t.place.json(), "gone": t.view.as_ref().is_some_and(View::is_gone)})).collect::<Vec<_>>(),
            "sections": sections,
            "lines": view.map_or(0, |v| v.outline().total_lines),
            "first": tab.map_or(0, |t| t.scroll.first),
            "visible": [sheet.visible.0, sheet.visible.1],
            "selected": selected,
            "bar": bar,
            "painted": painted,
            "work": {
                "outlines": sheet.work.outlines, "rows": sheet.work.rows, "cells": sheet.work.cells, "lines": sheet.work.lines,
                "evaluations": sheet.work.evaluations,
            },
            "edit": tab.and_then(|t| t.edit.as_ref()).map_or(Value::Null, EditState::json),
            "footer": self.sheet_footer_text(),
            "last": sheet.last.as_ref().map_or(Value::Null, Last::json),
            "changed": changed,
            "hscroll": tab.map_or(0.0, |t| f64::from(t.hscroll)),
            "columns": columns,
            "visualLines": view.map_or(0, |v| v.visual().total),
            "pairColumns": view.map_or(0, |v| v.visual().columns),
            "rowNames": sheet.row_names.iter().map(|(label, ink)| json!({"label": label, "ink": ink})).collect::<Vec<_>>(),
            "menu": sheet.menu.is_some(),
            "card": self.sheet_card_json(),
            "ops": self.sheet_ops_json(),
        })
    }

    // ---- the commands -------------------------------------------------------------------------------------------------------------------

    /// Whether a window command that acts on an open sheet can run now.
    pub(crate) fn sheet_acts_verdict(&self) -> std::result::Result<(), String> {
        if self.ui.sheet.tabs.is_empty() { Err("no ShapeSheet is open".to_owned()) } else { Ok(()) }
    }

    /// Whether `window.shapesheet` can open what the control with these `args` asks for (a JSON object, or empty): a drawing, and for Shape a shape selected.
    pub(crate) fn sheet_open_verdict(&self, args: &str) -> std::result::Result<(), String> {
        if self.session.document().is_none() {
            return Err("no document open".to_owned());
        }
        let wants_shape =
            serde_json::from_str::<Value>(args).ok().and_then(|v| v.get("of").and_then(Value::as_str).map(|of| of == "shape")).unwrap_or(false);
        if wants_shape && self.ui.edit.selection().is_empty() {
            return Err("nothing is selected".to_owned());
        }
        Ok(())
    }

    /// The box a View check box stands for, by the keys of its args: whether it is ticked.
    pub(crate) fn sheet_view_checked(&self, args: &str) -> bool {
        let v: Value = serde_json::from_str(args).unwrap_or(Value::Null);
        if v.get("formulas").is_some() {
            self.ui.sheet.formulas
        } else if v.get("values").is_some() {
            self.ui.sheet.values
        } else {
            false
        }
    }

    /// The params a press on a View check box sends: the box it stands for, the other way from how it is.
    pub(crate) fn sheet_view_press(&self, args: &str) -> Value {
        let v: Value = serde_json::from_str(args).unwrap_or(Value::Null);
        if v.get("formulas").is_some() { json!({"formulas": !self.ui.sheet.formulas}) } else { json!({"values": !self.ui.sheet.values}) }
    }

    /// Runs the window command `id` of the sheets.
    pub(crate) fn sheet_window_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        match id {
            "window.shapesheet" => {
                let place = self.sheet_place_for(p)?;
                self.ui.sheet.open(place);
                self.sheet_prepare();
                Ok(())
            }
            "window.sheetTab" => {
                let i = self.sheet_tab_param(id, p)?.ok_or_else(|| bad(id, "missing `tab`"))?;
                self.ui.sheet.activate(i);
                self.ui.sheet.focus = true;
                Ok(())
            }
            "window.sheetClose" => {
                let i = self.sheet_tab_param(id, p)?.unwrap_or(self.ui.sheet.active);
                self.ui.sheet.close(i);
                Ok(())
            }
            "window.sheetDock" => {
                let at = word(id, p, "at")?.ok_or_else(|| bad(id, "missing `at`"))?;
                let dock = Dock::from_word(at).ok_or_else(|| bad(id, "`at` must be below, beside or fill"))?;
                self.set_sheet_dock(dock);
                Ok(())
            }
            "window.sheetView" => {
                let (formulas, values) = (flag(id, p, "formulas")?, flag(id, p, "values")?);
                if formulas.is_none() && values.is_none() {
                    return Err(bad(id, "give `formulas`, `values` or both"));
                }
                let (f, v) = (formulas.unwrap_or(self.ui.sheet.formulas), values.unwrap_or(self.ui.sheet.values));
                if !f && !v {
                    return Err(bad(id, "at least one of Formulas and Values stays on"));
                }
                (self.ui.sheet.formulas, self.ui.sheet.values) = (f, v);
                Ok(())
            }
            "window.sheetSections" => self.sheet_sections_command(id, p),
            "window.sheetCollapse" => self.sheet_collapse_command(id, p),
            "window.sheetSelect" => self.sheet_select_command(id, p),
            "window.sheetEdit" => self.sheet_edit_command(id, p),
            "window.sheetInsertRow" => self.sheet_insert_row_command(id, p),
            "window.sheetDeleteRow" => self.sheet_delete_row_command(id),
            "window.sheetInsertSection" => self.sheet_insert_section_command(id, p),
            "window.sheetDeleteSection" => self.sheet_delete_section_command(id),
            "window.sheetEditFormula" => self.sheet_edit_formula_command(id, p),
            "window.sheetAddRow" => self.sheet_add_row_command(id, p),
            other => Err(EngineError::UnknownCommand(shown(other, 64))),
        }
    }

    /// Where the area docks. Fill remembers the dock it came from, which the drawing's own tab goes back to.
    pub(crate) fn set_sheet_dock(&mut self, dock: Dock) {
        let sheet = &mut self.ui.sheet;
        if dock == Dock::Fill && sheet.dock != Dock::Fill {
            sheet.back_to = sheet.dock;
        }
        sheet.dock = dock;
    }

    /// The tab a command's `tab` names: a place from 0 or a title. `None` when it gives none.
    fn sheet_tab_param(&self, id: &str, p: &Map<String, Value>) -> Result<Option<usize>> {
        let tabs = &self.ui.sheet.tabs;
        match p.get("tab") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => {
                let at =
                    n.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(id, "`tab` must be a whole number from 0, or a title"))?;
                if at < tabs.len() {
                    Ok(Some(at))
                } else {
                    Err(bad(id, format!("no tab {at}: the tabs are numbered from 0 to {}", tabs.len().saturating_sub(1))))
                }
            }
            Some(Value::String(title)) => tabs
                .iter()
                .position(|t| t.title.eq_ignore_ascii_case(title.trim()))
                .map(Some)
                .ok_or_else(|| bad(id, format!("no tab titled `{}`", shown(title, 64)))),
            Some(_) => Err(bad(id, "`tab` must be a whole number from 0, or a title")),
        }
    }

    /// The sheet `window.shapesheet` opens for its params (design 5.1), by the engine's rules of naming a sheet (design 7.1).
    fn sheet_place_for(&mut self, p: &Map<String, Value>) -> Result<Place> {
        const ID: &str = "window.shapesheet";
        let current = self.current_page();
        let Some(doc) = self.session.document() else { return Err(bad(ID, "no document open")) };
        let selected = self.ui.edit.selection().first().copied();
        let (of, id, page, style, master) =
            (word(ID, p, "of")?, whole(ID, p, "id")?, whole(ID, p, "page")?, whole(ID, p, "style")?, whole(ID, p, "master")?);
        let mut asked = Map::new();
        let mut put = |key: &str, value: Option<u32>| {
            if let Some(v) = value {
                asked.insert(key.to_owned(), json!(v));
            }
        };
        match of {
            // The big button: the shape selected, else the page on view.
            None if p.is_empty() => match (selected, current) {
                (Some(shape), page) => {
                    put("id", Some(shape));
                    put("page", self.ui.edit.selection_page().or(page));
                }
                (None, page) => put("page", page),
            },
            None => {
                put("id", id);
                put("page", page);
                put("style", style);
                put("master", master);
            }
            Some("shape") => match id.or(selected) {
                Some(shape) => {
                    put("id", Some(shape));
                    if master.is_some() {
                        put("master", master);
                    } else {
                        // The page the shape is on: the one named, else the page on view when it has the shape, else the first that has it.
                        let page = page.or_else(|| current.filter(|c| doc.find_shape(Owner::Page(*c), shape).is_some()));
                        put("page", page);
                    }
                }
                None => return Err(bad(ID, "nothing is selected: select a shape, or name it with `id`")),
            },
            Some("page") => put("page", page.or(current)),
            Some("document") => {
                asked.insert("document".to_owned(), json!(true));
            }
            Some("style") => match style {
                Some(_) => put("style", style),
                None => return Err(bad(ID, "`style` names the style: give its id")),
            },
            Some("master") => match master {
                Some(_) => {
                    put("master", master);
                    put("id", id);
                }
                None => return Err(bad(ID, "`master` names the master: give its id")),
            },
            Some(_) => return Err(bad(ID, "`of` must be shape, page, document, style or master")),
        }
        Place::from_params(doc, &Value::Object(asked)).map_err(|msg| bad(ID, msg))
    }

    fn sheet_sections_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        self.sheet_prepare();
        let sheet = &mut self.ui.sheet;
        let active = sheet.active;
        let Some(tab) = sheet.tabs.get_mut(active) else { return Err(bad(id, "no ShapeSheet is open")) };
        let Some(view) = tab.view.as_ref() else { return Err(bad(id, "the sheet is not in the drawing")) };
        let hidden: HashSet<String> = match p.get("shown") {
            None | Some(Value::Null) => HashSet::new(),
            Some(Value::Array(names)) => {
                let mut keep: HashSet<String> = HashSet::new();
                for name in names.iter().take(1024) {
                    let name = name.as_str().ok_or_else(|| bad(id, "`shown` must be an array of section names"))?;
                    let section = view.base().section(name).ok_or_else(|| bad(id, format!("no section {} in this sheet", shown(name, 48))))?;
                    keep.insert(section.name.clone());
                }
                view.base().sections.iter().map(|s| s.name.clone()).filter(|n| !keep.contains(n)).collect()
            }
            Some(_) => return Err(bad(id, "`shown` must be an array of section names")),
        };
        if hidden != tab.hidden {
            tab.hidden = hidden;
            tab.arrange_version += 1;
        }
        Ok(())
    }

    fn sheet_collapse_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        self.sheet_prepare();
        let name = word(id, p, "section")?.ok_or_else(|| bad(id, "missing `section`"))?;
        let open = flag(id, p, "open")?;
        let sheet = &mut self.ui.sheet;
        let active = sheet.active;
        let Some(tab) = sheet.tabs.get_mut(active) else { return Err(bad(id, "no ShapeSheet is open")) };
        let Some(view) = tab.view.as_ref() else { return Err(bad(id, "the sheet is not in the drawing")) };
        let section = view.base().section(name).ok_or_else(|| bad(id, format!("no section {} in this sheet", shown(name, 48))))?;
        let key = section.name.clone();
        let folded = tab.collapsed.contains(&key);
        let want_folded = match open {
            Some(open) => !open,
            None => !folded,
        };
        if want_folded != folded {
            if want_folded {
                tab.collapsed.insert(key);
            } else {
                tab.collapsed.remove(&key);
            }
            tab.arrange_version += 1;
        }
        Ok(())
    }

    /// `window.sheetEdit`: drives the editor as a person would. With `text` it begins an edit on the selected cell (or puts the text in the one open), `commit` commits it (the
    /// selection stays) and `cancel` gives it up. What the command answered is in the window state's `sheet.last`: a refusal is an answer, with its words, and the edit stays open.
    fn sheet_edit_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        let (text, commit, cancel) = (word(id, p, "text")?, flag(id, p, "commit")?.unwrap_or(false), flag(id, p, "cancel")?.unwrap_or(false));
        if commit && cancel {
            return Err(bad(id, "give commit or cancel, not both"));
        }
        self.sheet_prepare();
        let open = self.sheet_editing();
        if cancel {
            if !open {
                return Err(bad(id, "no edit is open"));
            }
            self.sheet_cancel_edit();
            return Ok(());
        }
        if commit && !open && text.is_none() {
            return Err(bad(id, "no edit is open: give `text` to begin one"));
        }
        match (open, text) {
            (false, Some(text)) => self.sheet_begin_edit(edit::Begin::Typing(text.to_owned())).map_err(|words| bad(id, words))?,
            (false, None) => self.sheet_begin_edit(edit::Begin::Text { in_bar: false }).map_err(|words| bad(id, words))?,
            (true, Some(text)) => {
                if let Some(e) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active).and_then(|t| t.edit.as_mut()) {
                    e.set_text(text.to_owned());
                    e.fresh = true;
                }
            }
            (true, None) => {}
        }
        if commit {
            self.sheet_commit_edit(edit::Then::Stay);
        }
        Ok(())
    }

    /// What the footer says: the command's words for a refused text while it stays in the editor, then what the last commit did for as long as the selection is where the commit
    /// left it, then where the selected cell comes from, then how big the sheet is.
    pub(crate) fn sheet_footer_text(&self) -> String {
        let Some(tab) = self.ui.sheet.tabs.get(self.ui.sheet.active) else { return String::new() };
        if let Some(words) = tab.edit.as_ref().and_then(|e| e.refusal.as_ref()) {
            return words.clone();
        }
        if let Some((sel, words)) = &tab.note
            && *sel == tab.selected
        {
            return words.clone();
        }
        match (&tab.bar, &tab.view) {
            (Some((_, _, words)), _) if !words.is_empty() => words.clone(),
            (_, Some(view)) if !view.is_gone() => format!("{} sections, {} lines", view.outline().sections.len(), view.outline().total_lines),
            _ => String::new(),
        }
    }

    fn sheet_select_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        // A person's press on another cell commits the edit that is open; a command does not decide that for a program: it says so.
        if self.sheet_editing() {
            return Err(bad(id, "an edit is open: commit or cancel it first (window.sheetEdit with commit or cancel)"));
        }
        self.sheet_prepare();
        let (cell, section, row) = (word(id, p, "cell")?, word(id, p, "section")?, p.get("row"));
        let sheet = &mut self.ui.sheet;
        let active = sheet.active;
        let Some(tab) = sheet.tabs.get_mut(active) else { return Err(bad(id, "no ShapeSheet is open")) };
        let Some(view) = tab.view.as_ref() else { return Err(bad(id, "the sheet is not in the drawing")) };
        let sel = select_target(id, view, cell, section, row)?;
        // A hidden section cannot be shown a selection in; a folded one is opened.
        if tab.hidden.contains(sel.section()) {
            return Err(bad(id, format!("{} is hidden: show it with window.sheetSections first", sel.section())));
        }
        if tab.collapsed.remove(sel.section()) {
            tab.arrange_version += 1;
        }
        tab.selected = Some(sel);
        tab.ensure_visible = true;
        sheet.focus = true;
        Ok(())
    }
}

/// How a row is named in the window state: a name, or a number as a reference spells it.
fn row_json(row: &RowKey) -> Value {
    match row {
        RowKey::Name(name) => json!(name.as_ref()),
        RowKey::Ix(ix) => json!(ix),
    }
}

/// The reference of the cell the bar shows, if it is a cell.
fn text_ref(text: &BarText) -> Option<String> {
    (!text.reference.is_empty()).then(|| text.reference.clone())
}

/// What the bar and the footer say of `sel`: the cell's reference, formula and value, and where it comes from. → the three, and whether the formula was cut by the reading.
fn bar_of(view: &mut View, doc: &diacraft_engine::doc::Document, sel: Option<&Sel>) -> (BarText, String, bool) {
    let Some(sel) = sel else { return (BarText::default(), String::new(), false) };
    match sel {
        Sel::Cell { .. } => match view.cell_of(doc, sel) {
            Some((_, cell)) => {
                let text = BarText {
                    reference: cell.reference.clone().unwrap_or_else(|| cell.name.clone()),
                    formula: cell.formula.clone(),
                    value: cell.shown.clone(),
                    error: cell.error.is_some(),
                };
                (text, cell::where_from(&cell), cell.truncated)
            }
            None => (BarText::default(), String::new(), false),
        },
        Sel::Row { section, row } => {
            let label = match row {
                RowKey::Name(name) => format!("{section}.{name}"),
                RowKey::Ix(ix) => format!("{section}.{}", ix.saturating_add(1)),
            };
            (BarText { reference: label, ..BarText::default() }, "A row is selected".to_owned(), false)
        }
        Sel::Header { section } => {
            (BarText { reference: section.clone(), ..BarText::default() }, "A section is selected: Space or Enter folds it".to_owned(), false)
        }
    }
}

/// The selection the params of `window.sheetSelect` name, in the sheet `view` holds: a reference, or a section with a cell and a row.
fn select_target(id: &str, view: &View, cell: Option<&str>, section: Option<&str>, row: Option<&Value>) -> Result<Sel> {
    let base = view.base();
    let not_there = |what: String| bad(id, format!("no {what} in this sheet"));
    // A row is a number as `sheet.read` gives it, or a name.
    let row_key = |info: &diacraft_engine::sheetview::SectionInfo, value: &Value| -> Result<RowKey> {
        let geometry = info.key.as_ref().is_some_and(|k| k.name.as_str() == "Geometry");
        let key = match value {
            Value::Number(n) => {
                let n = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(id, "`row` must be a whole number from 0, or a name"))?;
                RowKey::Ix(row_number_to_ix(geometry, n).ok_or_else(|| not_there(format!("row {n} in {}", info.label)))?)
            }
            Value::String(name) => RowKey::name(name),
            _ => return Err(bad(id, "`row` must be a whole number from 0, or a name")),
        };
        info.rows.contains(&key).then_some(key).ok_or_else(|| not_there(format!("row {} in {}", shown(&value.to_string(), 48), info.label)))
    };
    match (cell, section, row) {
        (None, None, None) => Err(bad(id, "give `cell` (a reference), or `section` with `row` and `cell`")),
        (Some(reference), None, None) => {
            let addr = diacraft_calc::cell_address(reference).map_err(|e| bad(id, format!("`cell` is not a reference: {e}")))?;
            let name = addr.cell.as_str();
            let info = match &addr.section {
                Some(key) => base.sections.iter().find(|s| s.key.as_ref() == Some(key)),
                None => base.sections.iter().find(|s| s.key.is_none() && s.cells.iter().any(|c| c.eq_ignore_ascii_case(name))),
            }
            .ok_or_else(|| not_there(format!("cell {}", shown(reference, 48))))?;
            match &addr.row {
                None if info.key.is_none() => {
                    let cell = info
                        .cells
                        .iter()
                        .find(|c| c.eq_ignore_ascii_case(name))
                        .ok_or_else(|| not_there(format!("cell {}", shown(reference, 48))))?;
                    Ok(Sel::Cell { section: info.name.clone(), row: None, cell: cell.clone() })
                }
                None => {
                    let flag = info
                        .flags
                        .iter()
                        .find(|c| c.eq_ignore_ascii_case(name))
                        .ok_or_else(|| not_there(format!("cell {}", shown(reference, 48))))?;
                    Ok(Sel::Cell { section: info.name.clone(), row: None, cell: (*flag).to_owned() })
                }
                Some(row) => {
                    if !info.rows.contains(row) {
                        return Err(not_there(format!("cell {}", shown(reference, 48))));
                    }
                    let column = info.columns.iter().find(|c| c.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_owned());
                    Ok(Sel::Cell { section: info.name.clone(), row: Some(row.clone()), cell: column })
                }
            }
        }
        (cell, Some(section), row) => {
            let info = base.section(section).ok_or_else(|| not_there(format!("section {}", shown(section, 48))))?;
            match (cell, row) {
                (_, Some(row)) => {
                    let key = row_key(info, row)?;
                    match cell {
                        None => Ok(Sel::Row { section: info.name.clone(), row: key }),
                        Some(cell) => {
                            let column = info
                                .columns
                                .iter()
                                .find(|c| c.eq_ignore_ascii_case(cell))
                                .ok_or_else(|| not_there(format!("cell {} in {}", shown(cell, 48), info.label)))?;
                            Ok(Sel::Cell { section: info.name.clone(), row: Some(key), cell: column.clone() })
                        }
                    }
                }
                (Some(cell), None) => {
                    let found = info.cells.iter().map(String::as_str).chain(info.flags.iter().copied()).find(|c| c.eq_ignore_ascii_case(cell));
                    let found = found.ok_or_else(|| not_there(format!("cell {} in {}", shown(cell, 48), info.label)))?;
                    Ok(Sel::Cell { section: info.name.clone(), row: None, cell: found.to_owned() })
                }
                (None, None) => Err(bad(id, "give `cell` or `row` with `section`")),
            }
        }
        (Some(_), None, Some(_)) | (None, None, Some(_)) => Err(bad(id, "`row` needs `section`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// While the sheet has the focus the shortcuts of the commands that change the drawing wait, and undo, redo and the window's own commands stay.
    #[test]
    fn the_shortcuts_that_change_the_drawing_wait_while_the_sheet_has_the_focus() {
        for id in ["edit.undo", "edit.redo", "file.save", "view.zoomIn", "window.find", "app.about", "tool.pointer"] {
            assert!(shortcut_allowed(id), "{id}");
        }
        for id in ["shape.delete", "edit.selectAll", "clipboard.copy", "clipboard.paste", "shape.group", "arrange.bringToFront", "edit.duplicate"] {
            assert!(!shortcut_allowed(id), "{id}");
        }
    }
}
