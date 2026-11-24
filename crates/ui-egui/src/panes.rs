//! The docking panes at the right of the drawing area: Size & Position, Format Shape, Pan & Zoom and
//! Drawing Explorer.
//!
//! The panes dock at the right of the drawing area, **several can stack there one above another, each with a title and a close button**,
//! and the title of Size & Position holds the selected shape's name. Two panes can **share a space, with tabs along the bottom** to switch
//! between them. A pane opens from View, Task Panes (`window.pane`), from the launcher of Shape Styles and the key F3 (Format Shape,
//! "a side pane opened from a shape's menu or F3") and from the context menu of the page; each pane's header has a menu (the docking choices: share a space with
//! another pane, or take a space of its own) and a close button.
//!
//! **What is kept** (per window, not in the drawing): which panes are open, in which spaces, and which tab of a space is on top ([`PanesState`]).
//! The panes read what the window already knows (the scene, the formatting of the selection, the view) and **every change they make is a command**: the
//! fields of Size & Position send `shape.move`, `shape.size`, `shape.rotate`, `shape.pin`, `shape.set_end` and `shape.set_length`, Format Shape sends the
//! window's `format.shape` (and `shape.format` with the ids of the shapes a typed weight or rounding was for), Pan & Zoom changes the view (`view.zoom`,
//! `view.pan`) and Drawing Explorer goes to a page (`page.select`). **What is only view state of a pane is not a command**: the Fill & Line and Effects
//! tabs of Format Shape.
//!
//! Choices no specification settles (ours): the stack shares the dock's height equally among its spaces; a pane takes a space of its own at the bottom of the
//! stack when it opens; the dock is as wide as the person drags it, from 200 points up to 45% of the window; a pane cannot float.

use egui::{Align2, CornerRadius, Frame, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::Value;

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg, text_width};

mod explorer;
mod fields;
mod format_fx;
mod format_shape;
mod pan_zoom;
mod shape_data;
mod size_position;

pub use explorer::Branch;
pub(crate) use fields::FieldState;
pub(crate) use pan_zoom::PanZoomState;

/// A pane of the dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    SizePosition,
    FormatShape,
    PanZoom,
    DrawingExplorer,
    /// The Shape Data window (`panes/shape_data.rs`).
    ShapeData,
    /// The Comments pane (`comments_ui.rs`).
    Comments,
}

impl Pane {
    /// Every pane, in the order the View tab lists them (then Format Shape and Drawing Explorer, which other places open, Shape Data, which the Data tab and the shape's menu open, and Comments, which the Review tab and the title bar open).
    pub const ALL: [Pane; 6] = [Pane::SizePosition, Pane::FormatShape, Pane::PanZoom, Pane::DrawingExplorer, Pane::ShapeData, Pane::Comments];

    /// The word the window's commands and the elements of `ui.elements` call it.
    pub const fn word(self) -> &'static str {
        match self {
            Pane::SizePosition => "size_position",
            Pane::FormatShape => "format_shape",
            Pane::PanZoom => "pan_zoom",
            Pane::DrawingExplorer => "drawing_explorer",
            Pane::ShapeData => "shape_data",
            Pane::Comments => "comments",
        }
    }

    /// The pane's title.
    pub const fn title(self) -> &'static str {
        match self {
            Pane::SizePosition => "Size & Position",
            Pane::FormatShape => "Format Shape",
            Pane::PanZoom => "Pan & Zoom",
            Pane::DrawingExplorer => "Drawing Explorer",
            Pane::ShapeData => "Shape Data",
            Pane::Comments => "Comments",
        }
    }

    /// The pane that is called `name`, by its word or its title, in any case.
    pub fn from_name(name: &str) -> Option<Pane> {
        let name = name.trim();
        Pane::ALL.into_iter().find(|p| p.word().eq_ignore_ascii_case(name) || p.title().eq_ignore_ascii_case(name))
    }
}

/// A space of the dock: one pane, or several that share it with tabs along the bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Space {
    pub panes: Vec<Pane>,
    /// The tab on top.
    pub active: usize,
}

impl Space {
    fn on_top(&self) -> Option<Pane> {
        self.panes.get(self.active).copied()
    }
}

/// What the window remembers of its panes: the spaces from the top of the stack down, and what the panes keep between frames.
#[derive(Default)]
pub struct PanesState {
    pub spaces: Vec<Space>,
    /// The number field being typed in, and the words about the one that was refused (`fields.rs`).
    pub(crate) fields: FieldState,
    /// The last answer of `shape.placement`, which Size & Position shows (`size_position.rs`).
    pub(crate) size: size_position::PlacementCache,
    /// The tab of Format Shape and the colours its Solid choices start from (`format_shape.rs`).
    pub(crate) format: format_shape::FormatState,
    /// What Pan & Zoom is doing with the pointer (`pan_zoom.rs`).
    pub(crate) pan_zoom: PanZoomState,
    /// The page of Drawing Explorer's tree last gone to, and the branches folded (`explorer.rs`).
    pub(crate) explorer: explorer::ExplorerState,
    /// The last `shapedata.list` the Shape Data window asked, and the variable list row being typed in (`shape_data.rs`).
    pub(crate) data: shape_data::DataCache,
}

/// The height of a space's header, in points.
const HEAD_H: f32 = 28.0;
/// The height of the tab row of a space that holds more than one pane.
const TABS_H: f32 = 26.0;
/// The room between two spaces.
const GAP: f32 = 4.0;
/// The narrowest and the default widest the dock is, in points.
const MIN_W: f32 = 200.0;
const MAX_W: f32 = 460.0;
/// How wide the dock is the first time it opens, kept as the person leaves it (a width that followed the window's width at the first frame would be
/// that of the small window the app starts in).
const DEFAULT_W: f32 = 280.0;

impl PanesState {
    /// Whether `pane` is open.
    pub fn is_open(&self, pane: Pane) -> bool {
        self.spaces.iter().any(|s| s.panes.contains(&pane))
    }

    /// The panes in the spaces from the top down, one list a space: what the window state says.
    pub fn layout(&self) -> Vec<Vec<&'static str>> {
        self.spaces.iter().map(|s| s.panes.iter().map(|p| p.word()).collect()).collect()
    }

    /// Opens `pane` in a space of its own at the bottom of the stack. A pane that is open stays where it is.
    pub fn open(&mut self, pane: Pane) {
        if !self.is_open(pane) {
            self.spaces.push(Space { panes: vec![pane], active: 0 });
        }
    }

    /// Closes `pane`: it leaves its space, and a space with nothing left goes.
    pub fn close(&mut self, pane: Pane) {
        for space in &mut self.spaces {
            if let Some(at) = space.panes.iter().position(|p| *p == pane) {
                space.panes.remove(at);
                space.active = space.active.min(space.panes.len().saturating_sub(1));
            }
        }
        self.spaces.retain(|s| !s.panes.is_empty());
    }

    /// Puts `pane` in the space `with` is in, as a tab and the one on top; or, with no `with`, in a space of its own at the bottom. `pane` is opened if it is not
    /// open. → false when `with` is not open (nothing changes).
    pub fn dock(&mut self, pane: Pane, with: Option<Pane>) -> bool {
        if let Some(with) = with
            && (with == pane || !self.is_open(with))
        {
            return with == pane && self.is_open(pane);
        }
        self.close(pane);
        match with.and_then(|w| self.spaces.iter().position(|s| s.panes.contains(&w))) {
            Some(at) => {
                if let Some(space) = self.spaces.get_mut(at) {
                    space.panes.push(pane);
                    space.active = space.panes.len() - 1;
                }
            }
            None => self.spaces.push(Space { panes: vec![pane], active: 0 }),
        }
        true
    }

    /// Puts `pane` on top of its space. → false when it is not open.
    pub fn raise(&mut self, pane: Pane) -> bool {
        for space in &mut self.spaces {
            if let Some(at) = space.panes.iter().position(|p| *p == pane) {
                space.active = at;
                return true;
            }
        }
        false
    }

    /// Moves the space `pane` is in one place up or down the stack.
    pub fn move_space(&mut self, pane: Pane, up: bool) {
        let Some(at) = self.spaces.iter().position(|s| s.panes.contains(&pane)) else { return };
        let to = if up { at.checked_sub(1) } else { Some(at + 1).filter(|n| *n < self.spaces.len()) };
        if let Some(to) = to {
            self.spaces.swap(at, to);
        }
    }
}

impl DiacraftApp {
    /// Whether `pane` is open.
    pub fn pane_open(&self, pane: Pane) -> bool {
        self.ui.panes.is_open(pane)
    }

    /// The panes open, a list of the panes of each space from the top down: what the window state says.
    pub fn pane_layout(&self) -> Vec<Vec<&'static str>> {
        self.ui.panes.layout()
    }

    /// `window.pane`: opens, closes or toggles a pane.
    pub(crate) fn pane_command(&mut self, name: &str, shown: Option<bool>) -> diacraft_engine::Result<()> {
        let pane = Pane::from_name(name).ok_or_else(|| {
            let names: Vec<&str> = Pane::ALL.iter().map(|p| p.word()).collect();
            diacraft_engine::EngineError::BadParams {
                cmd: "window.pane".to_owned(),
                msg: format!("no pane `{}`; the panes are: {}", diacraft_engine::shown(name, 32), names.join(", ")),
            }
        })?;
        match shown.unwrap_or(!self.ui.panes.is_open(pane)) {
            true => self.ui.panes.open(pane),
            false => self.ui.panes.close(pane),
        }
        Ok(())
    }

    /// `window.paneDock`: puts a pane in the space of another (tabs along the bottom), or in a space of its own.
    pub(crate) fn pane_dock_command(&mut self, name: &str, with: Option<&str>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.paneDock";
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: ID.to_owned(), msg };
        let pane = Pane::from_name(name).ok_or_else(|| bad(format!("no pane `{}`", diacraft_engine::shown(name, 32))))?;
        let with = match with {
            None => None,
            Some(w) => Some(Pane::from_name(w).ok_or_else(|| bad(format!("no pane `{}`", diacraft_engine::shown(w, 32))))?),
        };
        if !self.ui.panes.dock(pane, with) {
            return Err(bad(format!("`{}` is not open, so a pane cannot share its space", with.map_or("", Pane::title))));
        }
        Ok(())
    }

    /// `window.paneTab`: brings a pane that shares a space to the top of it.
    pub(crate) fn pane_tab_command(&mut self, name: &str) -> diacraft_engine::Result<()> {
        const ID: &str = "window.paneTab";
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: ID.to_owned(), msg };
        let pane = Pane::from_name(name).ok_or_else(|| bad(format!("no pane `{}`", diacraft_engine::shown(name, 32))))?;
        if !self.ui.panes.raise(pane) {
            return Err(bad(format!("`{}` is not open", pane.title())));
        }
        Ok(())
    }

    /// `window.paneMove`: moves the space of a pane one place up or down the stack.
    pub(crate) fn pane_move_command(&mut self, name: &str, up: bool) -> diacraft_engine::Result<()> {
        const ID: &str = "window.paneMove";
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: ID.to_owned(), msg };
        let pane = Pane::from_name(name).ok_or_else(|| bad(format!("no pane `{}`", diacraft_engine::shown(name, 32))))?;
        if !self.ui.panes.is_open(pane) {
            return Err(bad(format!("`{}` is not open", pane.title())));
        }
        self.ui.panes.move_space(pane, up);
        Ok(())
    }

    /// The dock at the right of the drawing area, when a pane is open. Drawn before the page tabs and the drawing area, so that the tabs and the page keep the room
    /// that is left.
    pub(crate) fn right_dock(&mut self, ui: &mut Ui, t: Tokens) {
        self.ui.panes.fields.next_frame();
        // (A value typed in a field is sent at the top of the frame, by `ui`, whether or not a pane is open.)
        self.forget_stale_refusal();
        if self.ui.panes.spaces.is_empty() {
            return;
        }
        let screen = ui.ctx().content_rect().width();
        let max = (screen * 0.45).clamp(MIN_W, MAX_W);
        let default = DEFAULT_W.clamp(MIN_W, max);
        let panel = egui::Panel::right("panes")
            .resizable(true)
            .default_size(default)
            .size_range(MIN_W.min(max)..=max)
            .show_separator_line(false)
            .frame(Frame::new().fill(t.panel_fill))
            .show(ui, |ui| self.dock_spaces(ui, &t));
        let rect = panel.response.rect;
        ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, t.separator));
        self.note("panes", rect, true);
    }

    fn dock_spaces(&mut self, ui: &mut Ui, t: &Tokens) {
        let area = ui.max_rect();
        // The spaces are laid in children of this ui, which add nothing to its used size: without this the panel would shrink to its narrowest.
        ui.set_min_size(area.size());
        let n = self.ui.panes.spaces.len().max(1);
        let each = ((area.height() - GAP * (n - 1) as f32) / n as f32).max(HEAD_H + 20.0);
        for index in 0..self.ui.panes.spaces.len() {
            let top = area.top() + index as f32 * (each + GAP);
            let rect = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), each));
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
            child.set_clip_rect(rect.intersect(area));
            self.draw_space(&mut child, t, index, rect);
            // The line between two spaces.
            if index + 1 < n {
                ui.painter().hline(area.x_range(), rect.bottom() + GAP / 2.0, Stroke::new(1.0, t.separator));
            }
        }
    }

    /// One space: the header of the pane on top, its body, and the tab row when the space holds more than one pane.
    fn draw_space(&mut self, ui: &mut Ui, t: &Tokens, index: usize, rect: Rect) {
        let Some(space) = self.ui.panes.spaces.get(index).cloned() else { return };
        let Some(pane) = space.on_top() else { return };
        let tabs = if space.panes.len() > 1 { TABS_H } else { 0.0 };
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD_H));
        let body = Rect::from_min_max(pos2(rect.left(), head.bottom()), pos2(rect.right(), (rect.bottom() - tabs).max(head.bottom())));
        self.note(format!("pane.{}", pane.word()), rect, true);
        self.pane_head(ui, t, pane, head);
        let mut inner = ui.new_child(UiBuilder::new().max_rect(body.shrink2(vec2(0.0, 0.0))));
        inner.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt(("pane_body", pane.word())).auto_shrink([false, false]).show(&mut inner, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 6, top: 8, bottom: 8 }).show(ui, |ui| {
                ui.set_width(body.width() - 28.0);
                match pane {
                    Pane::SizePosition => self.size_position_body(ui, t),
                    Pane::FormatShape => self.format_shape_body(ui, t),
                    Pane::PanZoom => self.pan_zoom_body(ui, t, body),
                    Pane::DrawingExplorer => self.explorer_body(ui, t),
                    Pane::ShapeData => self.shape_data_body(ui, t),
                    Pane::Comments => self.comments_body(ui, t),
                }
            });
        });
        if tabs > 0.0 {
            let row = Rect::from_min_max(pos2(rect.left(), rect.bottom() - tabs), rect.max);
            self.pane_tabs(ui, t, index, &space, row);
        }
    }

    /// The title, the menu of the docking choices and the close button.
    fn pane_head(&mut self, ui: &mut Ui, t: &Tokens, pane: Pane, head: Rect) {
        ui.painter().rect_filled(head, 0.0, t.panel_header_fill);
        ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, t.separator));
        let close = Rect::from_center_size(pos2(head.right() - 16.0, head.center().y), vec2(22.0, 22.0));
        let menu = Rect::from_center_size(pos2(close.left() - 14.0, head.center().y), vec2(22.0, 22.0));
        // The title, cut where the buttons begin.
        let title = self.pane_title(pane);
        let room = (menu.left() - head.left() - 14.0).max(0.0);
        let font = theme::font(size::TAB);
        let mut shown = title.clone();
        while shown.chars().count() > 4 && text_width(ui, &shown, &font) > room {
            shown = shown.chars().take(shown.chars().count().saturating_sub(2)).collect::<String>().trim_end().to_owned();
            if !shown.ends_with('\u{2026}') {
                shown.push('\u{2026}');
            }
            shown = shown.replace("\u{2026}\u{2026}", "\u{2026}");
        }
        ui.painter().text(pos2(head.left() + 10.0, head.center().y), Align2::LEFT_CENTER, &shown, font, t.text);
        let title_rect = Rect::from_min_max(head.min, pos2(menu.left() - 2.0, head.max.y));
        let resp = ui.interact(title_rect, ui.scope_id().with(("pane_title", pane.word())), Sense::hover());
        widgets::name_button(&resp, true, &title);
        self.note(format!("pane.{}.title", pane.word()), title_rect, true);
        // The docking menu.
        let resp = ui.interact(menu, ui.scope_id().with(("pane_menu", pane.word())), Sense::click());
        let words = format!("{} options", pane.title());
        widgets::name_button(&resp, true, &words);
        paint_button_bg(ui, menu, &resp, true, false, t, 3);
        for dx in [-4.5, 0.0, 4.5] {
            ui.painter().circle_filled(menu.center() + vec2(dx, 0.0), 1.4, t.text_dim);
        }
        self.note(format!("pane.{}.menu", pane.word()), menu, true);
        self.pane_menu(&resp.on_hover_text(words), pane);
        // The close button, an X drawn in code.
        let resp = ui.interact(close, ui.scope_id().with(("pane_close", pane.word())), Sense::click());
        let words = format!("Close {}", pane.title());
        widgets::name_button(&resp, true, &words);
        paint_button_bg(ui, close, &resp, true, false, t, 3);
        let (c, r) = (close.center(), 4.5);
        let stroke = Stroke::new(1.3, t.text);
        ui.painter().line_segment([c + vec2(-r, -r), c + vec2(r, r)], stroke);
        ui.painter().line_segment([c + vec2(-r, r), c + vec2(r, -r)], stroke);
        self.note(format!("pane.{}.close", pane.word()), close, true);
        if resp.on_hover_text(words).clicked() {
            let _ = self.execute("window.pane", &serde_json::json!({ "pane": pane.word(), "shown": false }));
        }
    }

    /// What the title of a pane says: its name, and for Size & Position the name of the shape selected.
    fn pane_title(&mut self, pane: Pane) -> String {
        match pane {
            Pane::SizePosition => self.size_position_title(),
            Pane::ShapeData => self.shape_data_title(),
            other => other.title().to_owned(),
        }
    }

    /// The menu of a pane's header: share a space with another pane, take a space of its own, move the space up or down.
    fn pane_menu(&mut self, resp: &egui::Response, pane: Pane) {
        let t = Tokens::get(&resp.ctx);
        let mut chosen: Option<Choice> = None;
        egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            ui.set_width(210.0);
            let layout = self.ui.panes.spaces.clone();
            let shared = layout.iter().find(|s| s.panes.contains(&pane)).is_some_and(|s| s.panes.len() > 1);
            let others: Vec<Pane> = Pane::ALL.into_iter().filter(|p| *p != pane && self.ui.panes.is_open(*p)).collect();
            for other in others {
                let words = format!("Share a space with {}", other.title());
                if self.pane_menu_row(ui, &t, &format!("pane.menu.share.{}", other.word()), &words, true) {
                    chosen = Some(Choice::Share(other));
                }
            }
            if self.pane_menu_row(ui, &t, "pane.menu.own", "Take a space of its own", shared) {
                chosen = Some(Choice::Own);
            }
            let at = layout.iter().position(|s| s.panes.contains(&pane)).unwrap_or(0);
            if self.pane_menu_row(ui, &t, "pane.menu.up", "Move the space up", at > 0) {
                chosen = Some(Choice::Up);
            }
            if self.pane_menu_row(ui, &t, "pane.menu.down", "Move the space down", at + 1 < layout.len()) {
                chosen = Some(Choice::Down);
            }
        });
        let sent = match chosen {
            Some(Choice::Share(other)) => Some(("window.paneDock", serde_json::json!({ "pane": pane.word(), "with": other.word() }))),
            Some(Choice::Own) => Some(("window.paneDock", serde_json::json!({ "pane": pane.word() }))),
            Some(Choice::Up) => Some(("window.paneMove", serde_json::json!({ "pane": pane.word(), "up": true }))),
            Some(Choice::Down) => Some(("window.paneMove", serde_json::json!({ "pane": pane.word(), "up": false }))),
            None => None,
        };
        if let Some((id, args)) = sent {
            let _ = self.execute(id, &args);
        }
    }

    fn pane_menu_row(&mut self, ui: &mut Ui, t: &Tokens, name: &str, label: &str, enabled: bool) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, label);
        paint_button_bg(ui, rect, &resp, enabled, false, t, 3);
        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), label_colour(t, enabled));
        self.note(name.to_owned(), rect, enabled);
        resp.clicked() && enabled
    }

    /// The tabs along the bottom of a space that holds several panes.
    fn pane_tabs(&mut self, ui: &mut Ui, t: &Tokens, index: usize, space: &Space, row: Rect) {
        ui.painter().rect_filled(row, 0.0, t.panel_header_fill);
        ui.painter().hline(row.x_range(), row.top(), Stroke::new(1.0, t.separator));
        let each = row.width() / space.panes.len() as f32;
        for (n, pane) in space.panes.iter().enumerate() {
            let r = Rect::from_min_size(pos2(row.left() + each * n as f32, row.top() + 1.0), vec2(each, row.height() - 1.0));
            let resp = ui.interact(r, ui.scope_id().with(("pane_tab", index, pane.word())), Sense::click());
            let active = n == space.active;
            widgets::name_toggle(&resp, true, active, pane.title());
            let round = CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 };
            if active {
                ui.painter().rect_filled(r.shrink2(vec2(2.0, 0.0)), round, t.panel_fill);
                ui.painter().rect_stroke(r.shrink2(vec2(2.0, 0.0)), round, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(r.shrink2(vec2(2.0, 0.0)), round, t.hover);
            }
            let clip = ui.painter().with_clip_rect(r.shrink2(vec2(6.0, 0.0)));
            clip.text(r.center(), Align2::CENTER_CENTER, pane.title(), theme::font(size::LABEL), if active { t.text } else { t.text_dim });
            self.note(format!("pane.tab.{}", pane.word()), r, true);
            if resp.clicked() && n != space.active {
                let _ = self.execute("window.paneTab", &serde_json::json!({ "pane": pane.word() }));
            }
        }
    }

    /// A drawing replaced the open one (an opened file, a new drawing): the edits that wait are dropped and every cache that was kept for a revision of the old
    /// drawing is forgotten, and the generation moves so that none of them is found again. **A session that is opened starts at revision 1 again**, so a cache
    /// keyed by the revision alone would show the old drawing's numbers for the new one until the first edit. The questions about saving the old drawing are
    /// closed too (`forget_save_questions`).
    pub(crate) fn drawing_replaced(&mut self) {
        self.ui.generation = self.ui.generation.wrapping_add(1);
        self.forget_save_questions();
        self.forget_edits();
        self.ui.panes.size = Default::default();
        self.ui.panes.pan_zoom.forget_picture();
        self.ui.panes.explorer.forget_tree();
        self.forget_shape_data_pane();
        self.ui.format.forget_looks();
    }

    /// The window state's word for the panes.
    pub(crate) fn panes_state(&self) -> Value {
        serde_json::json!(self.ui.panes.layout())
    }

    /// A check box of the View tab's Task Panes menu or the Developer tab: whether the pane its command names is open.
    pub(crate) fn pane_checked(&self, args: &str) -> bool {
        serde_json::from_str::<Value>(args)
            .ok()
            .and_then(|v| v.get("pane").and_then(Value::as_str).and_then(Pane::from_name))
            .is_some_and(|pane| self.ui.panes.is_open(pane))
    }
}

/// What a press in a pane's menu chose.
enum Choice {
    Share(Pane),
    Own,
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_open_close_and_share_a_space() {
        let mut s = PanesState::default();
        s.open(Pane::SizePosition);
        s.open(Pane::PanZoom);
        s.open(Pane::SizePosition);
        assert_eq!(s.layout(), [["size_position"], ["pan_zoom"]]);
        assert!(s.dock(Pane::PanZoom, Some(Pane::SizePosition)));
        assert_eq!(s.layout(), [vec!["size_position", "pan_zoom"]], "the second is a tab of the first, and on top");
        assert_eq!(s.spaces[0].active, 1);
        assert!(!s.dock(Pane::FormatShape, Some(Pane::DrawingExplorer)), "nothing to share with");
        assert!(!s.is_open(Pane::FormatShape));
        s.close(Pane::SizePosition);
        assert_eq!(s.layout(), [["pan_zoom"]]);
        assert!(s.dock(Pane::PanZoom, None));
        s.open(Pane::FormatShape);
        s.move_space(Pane::FormatShape, true);
        assert_eq!(s.layout(), [["format_shape"], ["pan_zoom"]]);
        s.move_space(Pane::FormatShape, true);
        assert_eq!(s.layout()[0], ["format_shape"], "the top space does not go higher");
        s.close(Pane::FormatShape);
        s.close(Pane::PanZoom);
        assert!(s.spaces.is_empty());
    }

    #[test]
    fn a_pane_is_found_by_its_word_or_its_title() {
        assert_eq!(Pane::from_name("size_position"), Some(Pane::SizePosition));
        assert_eq!(Pane::from_name(" Size & Position "), Some(Pane::SizePosition));
        assert_eq!(Pane::from_name("PAN & ZOOM"), Some(Pane::PanZoom));
        assert_eq!(Pane::from_name("nothing"), None);
        let mut words: Vec<&str> = Pane::ALL.iter().map(|p| p.word()).collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), Pane::ALL.len());
    }
}
