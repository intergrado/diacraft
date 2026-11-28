//! The lines of a sheet (design 5.5, 6): a list that paints **only the lines on view** and keeps its own scroll bar (`scroll.rs`).
//!
//! **A display group is name and value pairs in several columns**: a header line, then lines of pairs, filled down each column in turn (Shape Transform: Width, Height, Angle;
//! PinX, PinY, LocPinX, LocPinY; FlipX, FlipY, ResizeMode), each name right-aligned beside the box of its value; how many columns the width allows is `pairs.rs`'s. **A section of rows**
//! is a table: a header line, for Geometry its five own cells as pairs, a line of column titles, and one line a row, the Row column held at the left while the columns scroll sideways
//! under it. A column is as wide as the wider of its title and the widest box on view, from 56 to 240 points, and the Row column up to 200; **its right edge can be
//! dragged**, and a double click on the edge fits it again (tab state, not saved). **A row's name is red when its formula has an error**.
//!
//! **Editing.** The cell being edited has the editor over its box (`edit.rs`); a press on a cell, a double click and a right click are reported to the caller, which commits an open edit
//! first. **A text that is too long for its box is cut at the box's edge with no ellipsis** and is whole in the tooltip.
//!
//! **Cost.** One line height serves every line, measured from the real font (an egui row is as tall as its galley and not the font's `row_height`), so the first line on view is found by
//! arithmetic; the cells of the lines on view are resolved once (`view.rs`); a frame in which nothing changed resolves none. The widths of the columns are measured from the boxes on view
//! only.

use std::collections::{HashMap, HashSet};

use diacraft_engine::doc::Document;
use diacraft_engine::sheetview::{CellView, Line, LineKind, SectionInfo, Slot};
use egui::{Align2, CursorIcon, Id, Pos2, Rect, Role, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};

use super::cell::{self, CELL_PAD, Ink, Look, Painted};
use super::edit::{self, EditState};
use super::pairs::{VSlot, pair_columns};
use super::scroll::{self, SIXTEENTHS, Scroll};
use super::view::{Sel, View, Work};
use super::{LIST_ID, SheetTab};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, text_width};

/// The width of a scroll bar and the height of the horizontal one, in points.
pub const BAR: f32 = 12.0;

/// The narrowest and widest a column is by its content, and the widest the Row column is.
const COL_MIN: f32 = 56.0;
const COL_MAX: f32 = 240.0;
const ROW_COL_MAX: f32 = 200.0;

/// The narrowest and widest a column can be dragged to (an edge can be dragged).
pub const DRAG_MIN: f32 = 24.0;
pub const DRAG_MAX: f32 = 2_000.0;

/// How near a column's right edge a press takes its grip, on each side, in points.
const GRIP_IN: f32 = 5.0;
const GRIP_OUT: f32 = 3.0;

/// The width of the column of names of the pairs, between these.
const NAME_MIN: f32 = 80.0;
const NAME_MAX: f32 = 200.0;

/// The most boxes a frame records as painted.
const MAX_PAINTED: usize = 800;

/// The most row names a frame records.
const MAX_ROW_NAMES: usize = 400;

/// A control the grid drew, for `ui.elements`.
pub struct Note {
    pub name: String,
    pub rect: Rect,
    pub enabled: bool,
}

/// What the grid is given.
pub struct GridIn<'a> {
    pub t: &'a Tokens,
    pub formulas: bool,
    pub values: bool,
    pub doc: &'a Document,
    /// The cells the last commit worked out again on this sheet, by reference.
    pub changed: &'a HashSet<String>,
    /// The sheet has the focus: the grid holds the keyboard so that Tab and the arrows do not move egui's focus about.
    pub focused: bool,
}

/// What the grid did in the frame.
#[derive(Default)]
pub struct GridOut {
    /// The lines on view, from up to but not counting to.
    pub visible: (u64, u64),
    pub work: Work,
    pub notes: Vec<Note>,
    pub painted: Vec<Painted>,
    /// A press landed in the list: the sheet has the focus.
    pub pressed: bool,
    /// The sections whose header was pressed this frame.
    pub toggled: Vec<String>,
    /// A press chose this cell, row or header (the caller commits an open edit first).
    pub select: Option<Sel>,
    /// A double click: an edit of the selection begins.
    pub begin: bool,
    /// A right click: where, and on what.
    pub menu: Option<(Pos2, Option<Sel>)>,
    /// The names of the rows on view, with the ink of each (red for a row whose formula has an error).
    pub row_names: Vec<(String, &'static str)>,
    /// Where the selected cell was drawn.
    pub sel_rect: Option<Rect>,
    /// The cell being edited: its box and the part of it on view.
    edit_rect: Option<(Rect, Rect)>,
}

/// A column's edge being dragged: the column, how wide it was and where the pointer pressed.
#[derive(Clone, Debug, PartialEq)]
pub struct ColDrag {
    pub section: String,
    pub title: String,
    pub start: f32,
    pub press_x: f32,
}

/// The widths of the columns of a section of rows: the Row column, then one for each column.
#[derive(Clone, Debug, Default)]
struct Widths {
    row: f32,
    cols: Vec<f32>,
}

impl Widths {
    fn total(&self) -> f32 {
        self.row + self.cols.iter().sum::<f32>()
    }
}

/// A width held to the bounds of a column: finite for any input.
pub fn column_width(content: f32, min: f32, max: f32) -> f32 {
    if content.is_finite() { content.clamp(min, max.max(min)) } else { min }
}

/// The width a drag makes a column: where it began and how far the pointer went, held between the least and the most a column can be, finite for any input.
pub fn dragged_width(start: f32, delta: f32) -> f32 {
    let wanted = start + delta;
    if wanted.is_finite() { wanted.clamp(DRAG_MIN, DRAG_MAX) } else { DRAG_MIN }
}

fn pad(width: f32) -> f32 {
    width + 2.0 * CELL_PAD
}

/// A small chevron: pointing down when the section is open, right when it is folded.
fn chevron(painter: &egui::Painter, at: Pos2, open: bool, colour: egui::Color32) {
    let stroke = Stroke::new(1.5, colour);
    let (a, b, c) = if open {
        (at + vec2(-3.5, -1.5), at + vec2(0.0, 2.0), at + vec2(3.5, -1.5))
    } else {
        (at + vec2(-1.5, -3.5), at + vec2(2.0, 0.0), at + vec2(-1.5, 3.5))
    };
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, c], stroke);
}

/// The lines of one section on view and the widths their columns need: measured from the boxes on view only.
fn widths_of(ui: &Ui, lines: &[&Line], columns: usize, look: (bool, bool)) -> Widths {
    let font = cell::font();
    let strong = theme::font_strong(size::LABEL);
    let mut w = Widths { row: COL_MIN, cols: vec![COL_MIN; columns] };
    let mut row_content = 0.0_f32;
    let mut content = vec![0.0_f32; columns];
    for line in lines {
        match line.kind {
            LineKind::Row => {
                if let Some(row) = &line.row {
                    let label = row_label(row.label.as_str(), row.row_type.as_deref());
                    row_content = row_content.max(pad(text_width(ui, &label, &font)));
                }
                for c in &line.cells {
                    if let Some(slot) = c.col.and_then(|i| content.get_mut(i)) {
                        *slot = slot.max(cell::natural_width(ui, c, look.0, look.1));
                    }
                }
            }
            LineKind::Titles => {
                row_content = row_content.max(pad(text_width(ui, "Row", &strong)));
            }
            _ => {}
        }
    }
    w.row = column_width(row_content, COL_MIN, ROW_COL_MAX);
    for (slot, c) in w.cols.iter_mut().zip(content) {
        *slot = column_width(c, COL_MIN, COL_MAX);
    }
    w
}

/// What the Row column shows: the number or the name, and for a row that has a type, the type after it (`3  LineTo`).
pub fn row_label(label: &str, row_type: Option<&str>) -> String {
    match row_type {
        Some(kind) if !kind.is_empty() => format!("{label}  {kind}"),
        _ => label.to_owned(),
    }
}

/// Widens a column to its title too, then gives it the width a drag made it (done once the widths of the boxes are known).
fn with_titles(ui: &Ui, mut w: Widths, info: &SectionInfo, dragged: &HashMap<(String, String), f32>) -> Widths {
    let strong = theme::font_strong(size::LABEL);
    for (slot, title) in w.cols.iter_mut().zip(&info.columns) {
        *slot = column_width(slot.max(pad(text_width(ui, title, &strong))), COL_MIN, COL_MAX);
        if let Some(width) = dragged.get(&(info.name.clone(), title.clone())) {
            *slot = dragged_width(*width, 0.0);
        }
    }
    w
}

/// What one frame of the list shares between its lines.
struct Frame<'a> {
    t: &'a Tokens,
    body: Rect,
    line_h: f32,
    /// The width of the column of names of the pairs, and of one column of pairs.
    names_w: f32,
    pair_w: f32,
    hscroll: f32,
    /// The two View toggles.
    look: (bool, bool),
    id: Id,
    changed: &'a HashSet<String>,
}

/// The line being painted: its number, its place on the screen, and what the outline says of its section.
struct At<'a> {
    n: u64,
    row: Rect,
    visible: Rect,
    si: usize,
    info: &'a SectionInfo,
}

/// What the lines may change: the frame's account and the tab's column widths.
struct Acts<'a> {
    selected: &'a Option<Sel>,
    editing: Option<&'a Sel>,
    out: &'a mut GridOut,
    dragged: &'a mut HashMap<(String, String), f32>,
    drag: &'a mut Option<ColDrag>,
}

/// What the scroll of the frame came to.
struct Window {
    /// The lines on view: from up to but not counting to.
    range: (u64, u64),
    /// The most the first line can be.
    max_first: u64,
    /// A line's height, in sixteenths of a point.
    line_h16: u32,
    total: u64,
    /// How many whole lines fit.
    page: u64,
}

/// The scroll of the frame: moved by the wheel and by a cell chosen by a command or a key, and the lines it puts on view.
fn scroll_window(ui: &Ui, list: Rect, tab: &mut SheetTab, view_h: f32, line_h: f32) -> Window {
    let SheetTab { view, scroll: position, hscroll, selected, ensure_visible, .. } = tab;
    let total = view.as_ref().map_or(0, |v| v.visual().total);
    let line_h16 = u32::try_from((line_h * SIXTEENTHS as f32).round() as i64).unwrap_or(22 * SIXTEENTHS).max(1);
    let page = (view_h / line_h).floor().max(1.0) as u64;
    let max_first = scroll::max_first(total, page);
    // The wheel over the list, sideways and down.
    if ui.rect_contains_pointer(list) {
        let wheel = ui.input(|i| i.smooth_scroll_delta);
        if wheel.y != 0.0 {
            *position = position.by(-((wheel.y * SIXTEENTHS as f32).round() as i64), line_h16, max_first);
        }
        if wheel.x != 0.0 {
            *hscroll -= wheel.x;
        }
    }
    *position = position.clamped(max_first);
    // A cell chosen by a command or a key is brought on view.
    if std::mem::take(ensure_visible)
        && let Some(line) = selected.as_ref().and_then(|s| view.as_ref().and_then(|v| v.vline_of(s)))
    {
        if line < position.first {
            *position = Scroll::at(line);
        } else if line >= position.first.saturating_add(page) {
            *position = Scroll::at(line + 1 - page).clamped(max_first);
        }
    }
    let height16 = u32::try_from((view_h * SIXTEENTHS as f32).round() as i64).unwrap_or(u32::MAX);
    Window { range: scroll::visible_lines(position.first, position.offset, height16, line_h16, total), max_first, line_h16, total, page }
}

/// What is on view, by section: the width of the column of names of the pairs, and the widths of the columns of each section of rows.
fn measure(
    ui: &Ui,
    view: &View,
    range: (u64, u64),
    look: (bool, bool),
    pair_w: f32,
    dragged: &HashMap<(String, String), f32>,
) -> (f32, Vec<(usize, Widths)>) {
    let mut by_section: Vec<(usize, Vec<&Line>)> = Vec::new();
    let mut names_w = 0.0_f32;
    for n in range.0..range.1 {
        let Some((si, slot, engine)) = view.engine_lines_of(n) else { continue };
        for (_, el) in &engine {
            let Some(line) = view.line(*el) else { continue };
            match slot {
                VSlot::Pairs(_) => {
                    for c in &line.cells {
                        let name = c.reference.as_deref().unwrap_or(&c.name);
                        names_w = names_w.max(pad(text_width(ui, name, &cell::font())));
                    }
                }
                VSlot::Tail(_) => match by_section.last_mut() {
                    Some((last, lines)) if *last == si => lines.push(line),
                    _ => by_section.push((si, vec![line])),
                },
                VSlot::Header => {}
            }
        }
    }
    let names_w = column_width(names_w, NAME_MIN, NAME_MAX.min(pair_w * 0.45));
    let widths = by_section
        .iter()
        .filter_map(|(si, lines)| {
            let info = view.outline().sections.get(*si)?;
            Some((*si, with_titles(ui, widths_of(ui, lines, info.columns.len(), look), info, dragged)))
        })
        .collect();
    (names_w, widths)
}

/// The room of the list in a frame: the list's own scroll bars take the right and the bottom of it. → the area the lines are laid out in, whether the horizontal bar shows.
fn room_of(list: Rect, content_w: f32, line_h: f32) -> (Rect, bool) {
    let hbar = content_w > list.width() - BAR + 0.5;
    let view_w = (list.width() - BAR).max(1.0);
    let view_h = (list.height() - if hbar { BAR } else { 0.0 }).max(line_h);
    (Rect::from_min_size(list.min, vec2(view_w, view_h)), hbar)
}

/// The grid holds egui's keyboard focus while the sheet has the focus and no edit is open, with a filter that keeps Tab, the arrows and Escape from moving it elsewhere; the sheet's
/// keys (`keys.rs`) read them from the frame's input. It gives the focus up when the sheet loses it.
fn hold_focus(ui: &Ui, id: Id, focused: bool, editing: bool) {
    let ctx = ui.ctx();
    if focused && !editing {
        ctx.memory_mut(|m| {
            if m.focused().is_none() {
                m.request_focus(id);
            }
            m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true });
        });
    } else if ctx.memory(|m| m.has_focus(id)) {
        ctx.memory_mut(|m| m.surrender_focus(id));
    }
}

/// Draws the lines of the active tab in `list`: reads the cells of the lines on view, paints them, takes the presses and the wheel. A tab with no view draws nothing.
pub fn paint(ui: &mut Ui, list: Rect, tab: &mut SheetTab, input: GridIn<'_>) -> GridOut {
    let GridIn { t, formulas, values, doc, changed, focused } = input;
    let mut out = GridOut::default();
    if tab.view.is_none() {
        return out;
    }
    let line_h = cell::box_height(ui);
    let id = ui.scope_id().with(LIST_ID);

    let (body, hbar) = room_of(list, tab.content_w, line_h);
    let (view_w, view_h) = (body.width(), body.height());
    tab.view_w = view_w;
    hold_focus(ui, id.with("grid"), focused, tab.edit.is_some());

    // The grid is one control of its own, named for assistive technology, which takes the press that lands between boxes.
    let whole = ui.interact(body, id.with("grid"), Sense::click());
    whole.widget_info(|| WidgetInfo::labeled(Role::Grid, true, "ShapeSheet"));
    out.pressed = ui.input(|i| i.pointer.any_pressed()) && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| body.contains(p));
    if whole.secondary_clicked()
        && let Some(at) = whole.interact_pointer_pos()
    {
        out.menu = Some((at, tab.selected.clone()));
    }

    let window = scroll_window(ui, list, tab, view_h, line_h);
    out.visible = window.range;
    tab.page_lines = window.page;
    tab.page_w = view_w;
    let SheetTab { view, scroll: position, hscroll, selected, content_w, collapsed, edit, col_widths, col_drag, .. } = tab;
    let Some(view) = view.as_mut() else { return out };
    out.work = view.read_visual(doc, window.range.0, window.range.1);
    let view: &View = view;

    let columns = view.visual().columns.max(1);
    let pair_w = view_w / columns as f32;
    let (names_w, widths) = measure(ui, view, window.range, (formulas, values), pair_w, col_widths);
    let widest = widths.iter().map(|(_, w)| w.total()).fold(0.0_f32, f32::max);
    *content_w = widest;
    *hscroll = hscroll.clamp(0.0, (widest - view_w).max(0.0));
    // The width of the list changed how many columns of pairs fit: the next frame lays them out again.
    if pair_columns(view_w) != columns {
        ui.ctx().request_repaint();
    }

    let frame = Frame { t, body, line_h, names_w, pair_w, hscroll: *hscroll, look: (formulas, values), id, changed };
    let painter = ui.painter_at(body);
    let top = body.top() - position.offset as f32 / SIXTEENTHS as f32;
    let editing = edit.as_ref().map(|e| e.sel.clone());
    let mut acts = Acts { selected, editing: editing.as_ref(), out: &mut out, dragged: col_widths, drag: col_drag };
    for n in window.range.0..window.range.1 {
        let Some((si, vslot, engine)) = view.engine_lines_of(n) else { continue };
        let Some(info) = view.outline().sections.get(si) else { continue };
        let row = Rect::from_min_size(pos2(body.left(), top + (n - position.first) as f32 * line_h), vec2(view_w, line_h));
        let visible = row.intersect(body);
        if visible.height() <= 0.0 {
            continue;
        }
        painter.hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, t.group_divider));
        let at = At { n, row, visible, si, info };
        match vslot {
            VSlot::Header => paint_header(ui, &frame, &painter, &at, !collapsed.contains(&info.name), &mut acts),
            VSlot::Pairs(_) => paint_pairs(ui, &frame, &painter, &at, view, &engine, &mut acts),
            VSlot::Tail(_) => {
                let Some(line) = engine.first().and_then(|(_, el)| view.line(*el)) else { continue };
                let w = widths.iter().find(|(s, _)| *s == si).map(|(_, w)| w.clone()).unwrap_or_default();
                match line.kind {
                    LineKind::Titles => paint_titles(ui, &frame, &painter, &at, &w, &mut acts),
                    LineKind::Row => paint_row(ui, &frame, &painter, &at, (line, &w, view), &mut acts),
                    LineKind::Note => paint_note(&frame, &painter, &at, line, &mut acts),
                    LineKind::Header | LineKind::Cell => {}
                }
            }
        }
    }
    paint_editor(ui, &frame, edit.as_mut(), &mut out);
    paint_bars(ui, &frame, list, (view_h, hbar), (position, hscroll, widest), &window, &mut out);
    out
}

/// The editor over the cell being edited, grown to the right for a text that is wider than the cell, and cut at the list's edge.
fn paint_editor(ui: &mut Ui, fr: &Frame<'_>, edit: Option<&mut EditState>, out: &mut GridOut) {
    let (Some(edit), Some((full, visible))) = (edit.filter(|e| !e.in_bar && !e.in_card), out.edit_rect) else { return };
    if visible.width() <= 0.0 {
        return;
    }
    let wanted =
        text_width(ui, &edit.text.chars().take(cell::room_chars(fr.body.width())).collect::<String>(), &cell::font()) + 2.0 * CELL_PAD + 14.0;
    let width = full.width().max(wanted).min((fr.body.right() - full.left()).max(full.width()));
    let rect = Rect::from_min_size(full.min, vec2(width, full.height()));
    let clip = fr.body;
    ui.scope(|ui| {
        ui.set_clip_rect(clip.intersect(ui.clip_rect()));
        edit::show_editor(ui, rect, edit, fr.t, "Cell editor");
    });
    out.notes.push(Note { name: "sheet.edit".to_owned(), rect: rect.intersect(fr.body), enabled: true });
}

/// A section's header: its chevron, its title and how many rows it has; a press on it folds the section or opens it.
fn paint_header(ui: &Ui, fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, open: bool, acts: &mut Acts<'_>) {
    let (t, row, info) = (fr.t, at.row, at.info);
    let selected = matches!(acts.selected, Some(Sel::Header { section }) if section == &info.name);
    painter.rect_filled(row, 0.0, if selected { t.accent_soft } else { t.panel_header_fill });
    let resp = ui.interact(at.visible, fr.id.with(("section", at.n)), Sense::click());
    widgets::name_toggle(&resp, true, open, &info.label);
    if ui.rect_contains_pointer(at.visible) {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    chevron(painter, pos2(row.left() + 12.0, row.center().y), open, t.text);
    painter.text(pos2(row.left() + 24.0, row.center().y), Align2::LEFT_CENTER, &info.label, theme::font_strong(size::LABEL), t.text);
    if info.key.is_some() {
        let count = format!("{} {}", info.rows_total, if info.rows_total == 1 { "row" } else { "rows" });
        painter.text(pos2(row.right() - 8.0, row.center().y), Align2::RIGHT_CENTER, count, cell::font(), t.text_dim);
    }
    acts.out.notes.push(Note { name: format!("sheet.section.{}", info.name), rect: at.visible, enabled: true });
    if resp.clicked() {
        acts.out.toggled.push(info.name.clone());
        acts.out.select = Some(Sel::Header { section: info.name.clone() });
        acts.out.pressed = true;
    }
    // A right click on a header is the menu of its section (Insert Section, Delete Section), and does not fold it.
    if resp.secondary_clicked() {
        let sel = Sel::Header { section: info.name.clone() };
        acts.out.menu = Some((resp.interact_pointer_pos().unwrap_or(at.visible.left_bottom()), Some(sel)));
        acts.out.pressed = true;
    }
}

/// The line that says how many rows are not listed.
fn paint_note(fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, line: &Line, acts: &mut Acts<'_>) {
    let words = line.note.as_deref().unwrap_or("");
    painter.text(pos2(at.row.left() + 24.0, at.row.center().y), Align2::LEFT_CENTER, words, cell::font(), fr.t.text_dim);
    acts.out.notes.push(Note { name: "sheet.note".to_owned(), rect: at.visible, enabled: false });
}

/// A line of pairs: for each column the cell's name, right-aligned, and its box taking the rest of the column.
fn paint_pairs(ui: &Ui, fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, view: &View, engine: &[(Option<usize>, u64)], acts: &mut Acts<'_>) {
    let (t, row) = (fr.t, at.row);
    for (column, el) in engine {
        let (Some(c), Some(cell)) = (*column, view.line(*el).and_then(|l| l.cells.first())) else { continue };
        let left = row.left() + c as f32 * fr.pair_w;
        let right = (left + fr.pair_w).min(row.right());
        if c > 0 {
            painter.vline(left, row.y_range(), Stroke::new(1.0, t.group_divider));
        }
        // The name sits right-aligned beside its box: a name too long for its room is cut at the left.
        let names = Rect::from_min_max(pos2(left, row.top()), pos2((left + fr.names_w).min(right), row.bottom()));
        let name = cell.reference.as_deref().unwrap_or(&cell.name);
        let galley = cell::cut_tail(ui, name, t.text_dim, names.width());
        let x = names.right() - CELL_PAD - galley.size().x;
        painter.with_clip_rect(names.shrink2(vec2(CELL_PAD, 0.0)).intersect(painter.clip_rect())).galley(
            pos2(x, row.center().y - galley.size().y / 2.0),
            galley,
            t.text_dim,
        );
        let rect = Rect::from_min_max(pos2(names.right(), row.top()), pos2(right, row.bottom()));
        let sel = Some(Sel::Cell { section: at.info.name.clone(), row: None, cell: cell.name.clone() });
        cell_box(ui, fr, painter, at, (cell, rect, rect.intersect(fr.body)), &sel, acts);
    }
}

/// The line of column titles of a table: "Row" at the left, held there, and the columns' names scrolling under it, each with its edge to drag.
fn paint_titles(ui: &Ui, fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, w: &Widths, acts: &mut Acts<'_>) {
    let (t, row, body) = (fr.t, at.row, fr.body);
    painter.rect_filled(row, 0.0, t.panel_header_fill);
    let cols_left = body.left() + w.row;
    painter.text(pos2(row.left() + CELL_PAD, row.center().y), Align2::LEFT_CENTER, "Row", theme::font_strong(size::LABEL), t.text_dim);
    let name = format!("sheet.title.{}.Row", at.info.name);
    acts.out.notes.push(Note { name, rect: Rect::from_min_size(row.min, vec2(w.row, fr.line_h)).intersect(body), enabled: false });
    let columns = Rect::from_min_max(pos2(cols_left, row.top()), body.max);
    let clip = painter.with_clip_rect(columns.intersect(body));
    let mut x = cols_left - fr.hscroll;
    for (i, (title, cw)) in at.info.columns.iter().zip(&w.cols).enumerate() {
        if x + cw > cols_left && x < body.right() {
            let cell_clip = Rect::from_min_size(pos2(x + CELL_PAD, row.top()), vec2((cw - CELL_PAD).max(1.0), fr.line_h));
            let galley = cell::cut(ui, title, t.text_dim, cw - CELL_PAD);
            clip.with_clip_rect(cell_clip.intersect(clip.clip_rect())).galley(
                pos2(x + CELL_PAD, row.center().y - galley.size().y / 2.0),
                galley,
                t.text_dim,
            );
            let r = Rect::from_min_size(pos2(x, row.top()), vec2(*cw, fr.line_h)).intersect(columns);
            acts.out.notes.push(Note { name: format!("sheet.title.{}.{title}", at.info.name), rect: r, enabled: false });
            let grip =
                Rect::from_min_max(pos2(x + cw - GRIP_IN, row.top()), pos2(x + cw + GRIP_OUT, row.bottom())).intersect(columns).intersect(body);
            paint_grip(ui, fr, at, (i, title, *cw), grip, acts);
        }
        x += cw;
    }
}

/// The grip at a column's right edge: a drag changes the width, a double click fits the column to its content again.
fn paint_grip(ui: &Ui, fr: &Frame<'_>, at: &At<'_>, column: (usize, &String, f32), grip: Rect, acts: &mut Acts<'_>) {
    let (i, title, width) = column;
    if grip.width() <= 0.0 {
        return;
    }
    let resp = ui.interact(grip, fr.id.with(("grip", at.si, i)), Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::labeled(Role::Splitter, true, format!("Resize column {title}")));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
    }
    let key = (at.info.name.clone(), title.clone());
    if resp.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        *acts.drag = Some(ColDrag { section: key.0.clone(), title: key.1.clone(), start: width, press_x: origin.x });
    }
    if resp.dragged()
        && let (Some(drag), Some(now)) =
            (acts.drag.as_ref().filter(|d| d.section == key.0 && d.title == key.1), ui.input(|i| i.pointer.interact_pos()))
    {
        acts.dragged.insert(key.clone(), dragged_width(drag.start, now.x - drag.press_x));
        ui.ctx().request_repaint();
    }
    if resp.drag_stopped() {
        *acts.drag = None;
    }
    if resp.double_clicked() {
        acts.dragged.remove(&key);
        ui.ctx().request_repaint();
    }
}

/// A row of a table: the Row column (a press selects the row), then the boxes of its cells in their columns.
fn paint_row(ui: &Ui, fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, shown: (&Line, &Widths, &View), acts: &mut Acts<'_>) {
    let (t, row, body, info) = (fr.t, at.row, fr.body, at.info);
    let (line, w, view) = shown;
    let engine = line.line.saturating_sub(info.first_line);
    let slot = info.at(engine);
    let key_row = slot.and_then(|s| if let Slot::Row(i) = s { info.rows.get(i) } else { None });
    let row_selected = matches!((acts.selected.as_ref(), key_row), (Some(Sel::Row { section, row }), Some(r)) if section == &info.name && row == r);
    if row_selected {
        painter.rect_filled(row, 0.0, t.accent_soft);
    }
    if let Some(rv) = &line.row {
        paint_row_name(ui, fr, painter, at, (rv, key_row, row_selected, w.row), line, acts);
    }
    let cols_left = body.left() + w.row;
    let clip_rect = Rect::from_min_max(pos2(cols_left, row.top()), pos2(body.right(), row.bottom())).intersect(body);
    let clip = painter.with_clip_rect(clip_rect);
    for c in &line.cells {
        let Some(i) = c.col else { continue };
        let x = cols_left - fr.hscroll + w.cols.iter().take(i).sum::<f32>();
        let cw = w.cols.get(i).copied().unwrap_or(COL_MIN);
        if x + cw <= cols_left || x >= body.right() {
            continue;
        }
        let rect = Rect::from_min_size(pos2(x, row.top()), vec2(cw, fr.line_h));
        let sel = slot.and_then(|s| view.sel_at(at.si, s, Some(&c.name)));
        cell_box(ui, fr, &clip, at, (c, rect, rect.intersect(clip_rect)), &sel, acts);
    }
}

/// The Row column of a row: its name (**red when the row's formula has an error**; quiet for a row only a master or a style holds), a press on it selecting the row.
fn paint_row_name(
    ui: &Ui,
    fr: &Frame<'_>,
    painter: &egui::Painter,
    at: &At<'_>,
    shown: (&diacraft_engine::sheetview::RowView, Option<&diacraft_engine::doc::RowKey>, bool, f32),
    line: &Line,
    acts: &mut Acts<'_>,
) {
    let (t, row, body, info) = (fr.t, at.row, fr.body, at.info);
    let (rv, key_row, row_selected, width) = shown;
    let rect = Rect::from_min_size(row.min, vec2(width, fr.line_h));
    let text = row_label(&rv.label, rv.row_type.as_deref());
    let failing = line.cells.iter().any(|c| c.error.is_some());
    let ink = if failing {
        Ink::Error
    } else if rv.origin == "inherited" {
        Ink::Dim
    } else {
        Ink::Text
    };
    if acts.out.row_names.len() < MAX_ROW_NAMES {
        acts.out.row_names.push((rv.label.clone(), ink.word()));
    }
    // A named row's name is right-aligned and, too long for its column, cut at the left; a number sits at the left.
    let named = rv.key.is_string();
    let galley = if named { cell::cut_tail(ui, &text, ink.colour(t), width) } else { cell::cut(ui, &text, ink.colour(t), width) };
    let resp = ui.interact(rect.intersect(body), fr.id.with(("rowhead", at.n)), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::RowHeader, true, row_selected, format!("{}, row {text}", info.label)));
    let room = Rect::from_min_size(pos2(rect.left() + CELL_PAD, rect.top()), vec2((width - 2.0 * CELL_PAD).max(1.0), rect.height()));
    let x = if named { room.right() - galley.size().x } else { room.left() };
    painter.with_clip_rect(room.intersect(painter.clip_rect())).galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink.colour(t));
    acts.out.notes.push(Note { name: format!("sheet.row.{}.{}", info.name, rv.label), rect: rect.intersect(body), enabled: true });
    if resp.clicked()
        && let Some(r) = key_row
    {
        acts.out.select = Some(Sel::Row { section: info.name.clone(), row: r.clone() });
        acts.out.pressed = true;
    }
    // A right click on a row's number selects the row and opens the menu (Insert Row, Delete Row).
    if resp.secondary_clicked()
        && let Some(r) = key_row
    {
        let sel = Sel::Row { section: info.name.clone(), row: r.clone() };
        acts.out.menu = Some((resp.interact_pointer_pos().unwrap_or(rect.left_bottom()), Some(sel)));
        acts.out.pressed = true;
    }
}

/// The scroll bars over the list: the vertical one always, the horizontal one when a table is wider than the view.
fn paint_bars(ui: &mut Ui, fr: &Frame<'_>, list: Rect, room: (f32, bool), state: (&mut Scroll, &mut f32, f32), window: &Window, out: &mut GridOut) {
    let (view_h, hbar) = room;
    let (position, hscroll, widest) = state;
    let t = fr.t;
    let vtrack = Rect::from_min_max(pos2(list.right() - BAR, list.top()), pos2(list.right(), list.top() + view_h));
    let (page, pos) = (f64::from(view_h) / f64::from(fr.line_h), position.as_lines(window.line_h16));
    let lines = window.total as f64;
    let (next, resp) = scroll::scroll_bar(ui, fr.id.with("vbar"), vtrack, true, (lines, page, pos), "Sheet scroll bar", t);
    out.notes.push(Note { name: "sheet.scroll".to_owned(), rect: vtrack, enabled: lines > page });
    if (next - pos).abs() > f64::EPSILON && resp.is_pointer_button_down_on() {
        *position = Scroll::from_lines(next, window.line_h16, window.max_first);
    }
    if hbar {
        let htrack = Rect::from_min_max(pos2(list.left(), list.bottom() - BAR), pos2(list.right() - BAR, list.bottom()));
        let sizes = (f64::from(widest), f64::from(fr.body.width()), f64::from(*hscroll));
        let (next, resp) = scroll::scroll_bar(ui, fr.id.with("hbar"), htrack, false, sizes, "Sheet horizontal scroll bar", t);
        out.notes.push(Note { name: "sheet.hscroll".to_owned(), rect: htrack, enabled: true });
        if resp.is_pointer_button_down_on() {
            *hscroll = next as f32;
        }
    }
}

/// Draws one cell box, takes its press and records it. `rects` is the cell, its whole box and the part of it on view, and `sel` the selection the cell would be.
fn cell_box(ui: &Ui, fr: &Frame<'_>, painter: &egui::Painter, at: &At<'_>, rects: (&CellView, Rect, Rect), sel: &Option<Sel>, acts: &mut Acts<'_>) {
    // The box is laid out whole and the painter cuts it where the list ends; only the part on view takes a press.
    let (c, full, rect) = rects;
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let is_selected = sel.is_some() && sel == acts.selected;
    if is_selected {
        acts.out.sel_rect = Some(rect);
    }
    if sel.is_some() && sel.as_ref() == acts.editing {
        acts.out.edit_rect = Some((full, rect));
    }
    let resp = ui.interact(rect, fr.id.with(("cell", at.n)).with(&c.name), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Cell, true, is_selected, cell::accessible_name(&at.info.label, c)));
    let changed = c.reference.as_ref().is_some_and(|r| fr.changed.contains(r));
    let look = Look { formulas: fr.look.0, values: fr.look.1, selected: is_selected, changed };
    // A text too long for its box is cut at the box's edge: the painter is clipped to it.
    let record = cell::paint(ui, &painter.with_clip_rect(rect.intersect(painter.clip_rect())), fr.t, full, c, look);
    let reference = c.reference.as_deref().unwrap_or(&c.name);
    acts.out.notes.push(Note { name: format!("sheet.cell.{reference}"), rect, enabled: true });
    if acts.out.painted.len() < MAX_PAINTED {
        acts.out.painted.push(record);
    }
    if resp.clicked() {
        acts.out.select = sel.clone();
        acts.out.pressed = true;
    }
    if resp.double_clicked() {
        acts.out.begin = true;
    }
    if resp.secondary_clicked() {
        acts.out.menu = Some((resp.interact_pointer_pos().unwrap_or(rect.left_bottom()), sel.clone()));
        acts.out.pressed = true;
    }
    resp.on_hover_ui(|ui| {
        ui.set_max_width(420.0);
        ui.add(egui::Label::new(cell::tooltip(c)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_as_wide_as_its_content_between_the_bounds_and_finite_for_any_input() {
        assert_eq!(column_width(10.0, COL_MIN, COL_MAX), COL_MIN);
        assert_eq!(column_width(100.0, COL_MIN, COL_MAX), 100.0);
        assert_eq!(column_width(1000.0, COL_MIN, COL_MAX), COL_MAX);
        assert_eq!(column_width(f32::NAN, COL_MIN, COL_MAX), COL_MIN);
        assert_eq!(column_width(f32::INFINITY, COL_MIN, COL_MAX), COL_MIN, "an infinite width is none");
        assert_eq!(column_width(500.0, COL_MIN, ROW_COL_MAX), ROW_COL_MAX, "the Row column stops at 200");
        assert_eq!(column_width(5.0, 100.0, 50.0), 100.0, "a maximum under the minimum is the minimum");
    }

    #[test]
    fn a_dragged_column_is_as_wide_as_the_drag_makes_it_between_a_least_and_a_most_and_finite_for_any_input() {
        assert_eq!(dragged_width(100.0, 60.0), 160.0);
        assert_eq!(dragged_width(100.0, -400.0), DRAG_MIN, "no narrower than the least");
        assert_eq!(dragged_width(100.0, 1.0e9), DRAG_MAX, "no wider than the most");
        assert_eq!(dragged_width(f32::NAN, 5.0), DRAG_MIN);
        assert_eq!(dragged_width(100.0, f32::INFINITY), DRAG_MIN, "a width that is not a number is the least");
        assert_eq!(dragged_width(100.0, f32::NEG_INFINITY), DRAG_MIN);
    }

    #[test]
    fn the_row_column_shows_the_key_and_the_type_after_it() {
        assert_eq!(row_label("3", Some("LineTo")), "3  LineTo");
        assert_eq!(row_label("Prop.Cost", None), "Prop.Cost");
        assert_eq!(row_label("1", Some("")), "1");
    }

    #[test]
    fn the_list_keeps_its_room_for_the_scroll_bars() {
        let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0));
        let (body, hbar) = room_of(list, 100.0, 22.0);
        assert!(!hbar && body.width() == 400.0 - BAR && body.height() == 200.0);
        let (body, hbar) = room_of(list, 900.0, 22.0);
        assert!(hbar && body.height() == 200.0 - BAR, "a table wider than the view takes the bottom for its bar");
    }
}
