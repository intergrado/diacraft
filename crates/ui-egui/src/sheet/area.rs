//! The sheet area and the bar under the ribbon (design 5.2, 5.3).
//!
//! The area is **docked with the drawing**: **below** it by default, taking 45% of the middle band's height with a splitter between them (the drawing keeps at
//! least 120 points and the sheet 160), **beside** it at the right (left of the task panes), or **filling** the band, with a tab for the drawing's page at the left of the sheet tabs to
//! go back to it. A middle band narrower than 640 points shows Beside as Below. From the top the area is the tab strip (28 points), the lines, and the footer (22 points).
//!
//! The bar is a panel of its own, under the ribbon and over everything else, so that the whole window shifts once when the first sheet opens and once when the last closes.

use egui::{Align2, CornerRadius, Frame, Panel, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::bar::{self, BAR_H, BarIn, Press};
use super::edit::{Begin, Then};
use super::grid::{self, GridIn, Note};
use super::tabs::{self, TAB_H, TabAct, TabsIn};
use super::view::Sel;
use super::{Dock, gone_words};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg};

/// The height of the footer, in points.
const FOOTER_H: f32 = 22.0;

/// What the presses in the list asked for in a frame.
struct GridActs {
    select: Option<Sel>,
    begin: bool,
    menu: Option<(Pos2, Option<Sel>)>,
}

impl GridActs {
    fn menu_opened(&self) -> bool {
        self.menu.is_some()
    }
}

/// The least the sheet is tall, and the least the drawing keeps, in points (design 5.2).
const SHEET_MIN: f32 = 160.0;
const DRAWING_MIN: f32 = 120.0;

/// How much of the band the sheet takes when it opens, and the width of the band under which Beside is shown as Below.
const SHARE: f32 = 0.45;
const NARROW_BAND: f32 = 640.0;

/// The drawing's rulers and page tabs sit in the band as well: the room the drawing keeps is that and its least.
const DRAWING_FRAME: f32 = 22.0 + 28.0;

/// The dock that is drawn for the one chosen, in a band `band_width` points wide: Beside is shown as Below in a narrow band.
pub fn dock_for(chosen: Dock, band_width: f32) -> Dock {
    if chosen == Dock::Beside && band_width < NARROW_BAND { Dock::Below } else { chosen }
}

/// The height the sheet is given at first in a band `band` points tall, and the most it may be given: 45% of the band and no less than 160 points, and the drawing keeps 120.
pub fn below_sizes(band: f32) -> (f32, f32) {
    let band = if band.is_finite() { band.max(0.0) } else { 0.0 };
    let most = (band - DRAWING_MIN - DRAWING_FRAME).max(SHEET_MIN);
    ((band * SHARE).clamp(SHEET_MIN, most), most)
}

/// The width the sheet is given at first beside the drawing in a band `band` wide, and the most it may be given.
pub fn beside_sizes(band: f32) -> (f32, f32) {
    let band = if band.is_finite() { band.max(0.0) } else { 0.0 };
    let most = (band - 240.0).max(320.0);
    ((band * SHARE).clamp(320.0, most), most)
}

impl DiacraftApp {
    /// The page the drawing area shows, by its name: the drawing's tab in Fill.
    fn sheet_drawing_name(&mut self) -> String {
        let page = self.current_page();
        self.session()
            .document()
            .and_then(|d| page.and_then(|p| d.page(p)).map(|p| diacraft_engine::shown(&p.name, 40)))
            .unwrap_or_else(|| "Drawing".to_owned())
    }

    /// The formula bar, under the ribbon body, while a sheet is open.
    pub(crate) fn sheet_bar_panel(&mut self, ui: &mut Ui, t: Tokens) {
        if self.ui.sheet.tabs.is_empty() {
            self.ui.sheet.bar = None;
            return;
        }
        self.sheet_prepare();
        let active = self.ui.sheet.active;
        let text = self.ui.sheet.tabs.get(active).and_then(|tab| tab.bar.as_ref()).map(|(_, text, _)| text.clone()).unwrap_or_default();
        // The edit is the tab's; the bar edits it while it draws, and it goes back where it was.
        let mut edit = self.ui.sheet.tabs.get_mut(active).and_then(|tab| tab.edit.take());
        let panel = Panel::top("sheet_bar").exact_size(BAR_H).show_separator_line(false).frame(Frame::new().fill(t.panel_fill)).show(ui, |ui| {
            let rect = ui.max_rect();
            bar::paint(ui, rect, &t, BarIn { text: &text, edit: edit.as_mut() })
        });
        let words = edit.as_ref().and_then(|e| e.refusal.as_ref().map(|r| e.fault.as_ref().map_or_else(|| r.clone(), |f| f.words.clone())));
        if let Some(tab) = self.ui.sheet.tabs.get_mut(active) {
            tab.edit = edit;
        }
        let bar_rect = panel.response.rect;
        self.ui.sheet.bar = Some(bar_rect);
        let out = panel.inner;
        for n in out.notes {
            self.note(n.name, n.rect, n.enabled);
        }
        // What is wrong with a refused text, in a line under the bar (over what lies below: nothing moves when it comes and goes).
        if let Some(words) = words {
            let at = bar::fault_line(ui.ctx(), bar_rect, &t, &words);
            self.note("sheet.bar.fault", at, false);
        }
        match out.press {
            Some(Press::Cancel) => self.sheet_cancel_edit(),
            Some(Press::Accept) => {
                self.sheet_commit_edit(Then::Stay);
            }
            Some(Press::Begin) => self.sheet_bar_begin(),
            None => {}
        }
        if out.press.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// A press in the bar's text: an edit of the selected cell's formula begins there, or the one open in the cell moves to the bar.
    fn sheet_bar_begin(&mut self) {
        if let Some(edit) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active).and_then(|t| t.edit.as_mut()) {
            edit.in_bar = true;
            edit.fresh = true;
        } else {
            self.sheet_try_begin(Begin::Text { in_bar: true });
        }
    }

    /// The sheet area, docked as chosen. → whether it fills the band, so that the drawing and its page tabs are not drawn.
    pub(crate) fn sheet_area(&mut self, ui: &mut Ui, t: Tokens) -> bool {
        if self.ui.sheet.tabs.is_empty() {
            self.ui.sheet.area = None;
            self.ui.sheet.shown_dock = self.ui.sheet.dock;
            return false;
        }
        self.sheet_prepare();
        let band = ui.available_rect_before_wrap();
        let dock = dock_for(self.ui.sheet.dock, band.width());
        self.ui.sheet.shown_dock = dock;
        let frame = Frame::new().fill(t.panel_fill);
        match dock {
            Dock::Below => {
                let (first, most) = below_sizes(band.height());
                let panel = Panel::bottom("sheet_below")
                    .resizable(true)
                    .default_size(first)
                    .size_range(SHEET_MIN.min(most)..=most)
                    .show_separator_line(false)
                    .frame(frame)
                    .show(ui, |ui| {
                        let rect = ui.max_rect();
                        ui.set_min_size(rect.size());
                        self.sheet_body(ui, &t, rect);
                    });
                let rect = panel.response.rect;
                ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, t.separator));
                self.ui.sheet.area = Some(rect);
                self.note("sheet.area", rect, true);
                false
            }
            Dock::Beside => {
                let (first, most) = beside_sizes(band.width());
                let panel = Panel::right("sheet_beside")
                    .resizable(true)
                    .default_size(first)
                    .size_range(320.0_f32.min(most)..=most)
                    .show_separator_line(false)
                    .frame(frame)
                    .show(ui, |ui| {
                        let rect = ui.max_rect();
                        ui.set_min_size(rect.size());
                        self.sheet_body(ui, &t, rect);
                    });
                let rect = panel.response.rect;
                ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, t.separator));
                self.ui.sheet.area = Some(rect);
                self.note("sheet.area", rect, true);
                false
            }
            Dock::Fill => {
                let panel = egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                    let rect = ui.max_rect();
                    self.sheet_body(ui, &t, rect);
                });
                let rect = panel.response.rect;
                self.ui.sheet.area = Some(rect);
                self.note("sheet.area", rect, true);
                true
            }
        }
    }

    /// The inside of the area: the tab strip, the lines of the tab on top (or the words of a sheet that is gone), and the footer.
    fn sheet_body(&mut self, ui: &mut Ui, t: &Tokens, rect: Rect) {
        let strip = Rect::from_min_size(rect.min, vec2(rect.width(), TAB_H));
        let footer = Rect::from_min_max(pos2(rect.left(), (rect.bottom() - FOOTER_H).max(strip.bottom())), rect.max);
        let list = Rect::from_min_max(pos2(rect.left(), strip.bottom()), pos2(rect.right(), footer.top()));
        let (titles, active, dock) = {
            let s = &self.ui.sheet;
            (s.tabs.iter().map(|tab| tab.title.clone()).collect::<Vec<_>>(), s.active, s.shown_dock)
        };
        let drawing = (dock == Dock::Fill).then(|| self.sheet_drawing_name());
        let scroll_to_active = self.ui.sheet.scrolled_to != Some(active);
        let input = TabsIn { titles: &titles, active, dock: self.ui.sheet.dock, drawing: drawing.as_deref(), scroll_to_active };
        let (acts, notes) = tabs::paint(ui, strip, t, &input);
        self.ui.sheet.scrolled_to = Some(active);
        for n in notes {
            self.note(n.name, n.rect, n.enabled);
        }

        let gone = self.ui.sheet.tabs.get(active).and_then(|tab| tab.view.as_ref()).is_some_and(|v| v.is_gone());
        let mut notes: Vec<Note> = Vec::new();
        let mut grid_acts: Option<GridActs> = None;
        if gone {
            self.sheet_gone(ui, t, list, &mut notes);
        } else {
            let doc = self.session.document();
            let sheet = &mut self.ui.sheet;
            let (formulas, values, focused) = (sheet.formulas, sheet.values, sheet.focus);
            let changed = sheet.tabs.get(active).map(|tab| sheet.changed_on(&tab.place.json())).unwrap_or_default();
            if let (Some(doc), Some(tab)) = (doc, sheet.tabs.get_mut(active)) {
                let out = grid::paint(ui, list, tab, GridIn { t, formulas, values, doc, changed: &changed, focused });
                for name in &out.toggled {
                    if !tab.collapsed.remove(name) {
                        tab.collapsed.insert(name.clone());
                    }
                    tab.arrange_version += 1;
                }
                if !out.toggled.is_empty() {
                    ui.ctx().request_repaint();
                }
                tab.sel_rect = out.sel_rect;
                sheet.visible = out.visible;
                sheet.work.cells += out.work.cells;
                sheet.work.lines += out.work.lines;
                sheet.painted = out.painted;
                sheet.row_names = out.row_names;
                if out.pressed {
                    sheet.focus = true;
                }
                notes = out.notes;
                grid_acts = Some(GridActs { select: out.select, begin: out.begin, menu: out.menu });
            }
            notes.push(Note { name: "sheet.list".to_owned(), rect: list, enabled: true });
        }
        for n in notes {
            self.note(n.name, n.rect, n.enabled);
        }
        self.sheet_footer(ui, t, footer);
        // A menu opened by an earlier frame is drawn before a press of this one opens another.
        self.sheet_menu(ui.ctx(), t);
        if let Some(acts) = grid_acts {
            self.sheet_grid_acts(acts, ui.ctx());
        }
        for act in acts {
            self.sheet_tab_act(act, ui.ctx());
        }
    }

    /// The words of a sheet whose object is gone, and a button that closes its tab (design 5.1).
    fn sheet_gone(&mut self, ui: &mut Ui, t: &Tokens, list: Rect, notes: &mut Vec<Note>) {
        let words = self.ui.sheet.tabs.get(self.ui.sheet.active).map_or("", |tab| gone_words(&tab.place));
        let centre = list.center();
        let text = Rect::from_center_size(centre - vec2(0.0, 16.0), vec2(list.width().min(420.0), 20.0));
        let resp = ui.interact(text, ui.scope_id().with("sheet_gone"), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words));
        ui.painter().text(text.center(), Align2::CENTER_CENTER, words, theme::font(size::TAB), t.text);
        let button = Rect::from_center_size(centre + vec2(0.0, 18.0), vec2(80.0, 26.0));
        let close = ui.interact(button, ui.scope_id().with("sheet_gone_close"), Sense::click());
        widgets::name_button(&close, true, "Close");
        paint_button_bg(ui, button, &close, true, false, t, 4);
        ui.painter().rect_stroke(button, CornerRadius::same(4), Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
        ui.painter().text(button.center(), Align2::CENTER_CENTER, "Close", theme::font(size::LABEL), t.text);
        notes.push(Note { name: "sheet.gone".to_owned(), rect: text, enabled: false });
        notes.push(Note { name: "sheet.gone.close".to_owned(), rect: button, enabled: true });
        if close.clicked() {
            let active = self.ui.sheet.active;
            self.ui.sheet.close(active);
            ui.ctx().request_repaint();
        }
    }

    /// The footer: where the selected cell comes from, or how many sections and lines the sheet has.
    fn sheet_footer(&mut self, ui: &mut Ui, t: &Tokens, footer: Rect) {
        let painter = ui.painter_at(footer);
        painter.rect_filled(footer, 0.0, t.panel_header_fill);
        painter.hline(footer.x_range(), footer.top() + 0.5, Stroke::new(1.0, t.separator));
        let text = self.sheet_footer_text();
        let ink = if self.ui.sheet.tabs.get(self.ui.sheet.active).is_some_and(|t| t.edit.as_ref().is_some_and(|e| e.refusal.is_some())) {
            t.sheet_error
        } else {
            t.text_dim
        };
        let galley = super::cell::elided(ui, &text, ink, footer.width() - 16.0);
        painter.galley(pos2(footer.left() + 8.0, footer.center().y - galley.size().y / 2.0), galley, ink);
        // The footer's words are whole in its tooltip.
        let resp = ui.interact(footer, ui.scope_id().with("sheet_footer"), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("Status: {text}")));
        resp.on_hover_text(text);
        self.note("sheet.footer", footer, false);
    }

    /// The sheet's right-click menu (opened by a right click or Shift+F10): Formulas and Values, the View group's two toggles (the command `window.sheetView`). It closes on a press
    /// elsewhere, on Escape and on an item.
    fn sheet_menu(&mut self, ctx: &egui::Context, t: &Tokens) {
        let Some(at) = self.ui.sheet.menu else { return };
        let (formulas, values) = (self.ui.sheet.formulas, self.ui.sheet.values);
        let mut chosen: Option<&'static str> = None;
        let mut seen: Vec<(&'static str, Rect)> = Vec::new();
        let area = egui::Area::new(egui::Id::unique("sheet_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.panel_fill).show(ui, |ui| {
                ui.set_min_width(140.0);
                for (label, on, key) in [("Formulas", formulas, "formulas"), ("Values", values, "values")] {
                    // A check box: the tick says which of the two views are on.
                    let mut ticked = on;
                    let item = ui.checkbox(&mut ticked, label);
                    seen.push((label, item.rect));
                    if item.clicked() {
                        chosen = Some(key);
                    }
                }
            });
        });
        for (label, rect) in seen {
            self.note(format!("sheet.menu.{label}"), rect, true);
        }
        let close = chosen.is_some() || area.response.clicked_elsewhere();
        if let Some(key) = chosen {
            let on = if key == "formulas" { formulas } else { values };
            if let Err(e) = self.execute("window.sheetView", &serde_json::json!({ key: !on })) {
                self.set_notice(&e.to_string());
            }
        }
        if close {
            self.ui.sheet.menu = None;
            ctx.request_repaint();
        }
    }

    /// What the grid's presses asked for: a cell chosen (an open edit is committed first, and stays when the command refuses it), a double click that begins an edit, a right
    /// click that opens the menu.
    fn sheet_grid_acts(&mut self, acts: GridActs, ctx: &egui::Context) {
        let active = self.ui.sheet.active;
        let (begin, opened) = (acts.begin, acts.menu_opened());
        if let Some(sel) = acts.select {
            let elsewhere = self.ui.sheet.tabs.get(active).and_then(|t| t.edit.as_ref()).is_some_and(|e| e.sel != sel);
            let closed = !elsewhere || self.sheet_commit_edit(Then::Stay);
            if closed
                && let Some(tab) = self.ui.sheet.tabs.get_mut(active)
                && tab.edit.is_none()
            {
                tab.selected = Some(sel);
            }
        }
        if begin {
            self.sheet_try_begin(Begin::Text { in_bar: false });
        }
        if let Some((at, sel)) = acts.menu {
            if let (Some(sel), Some(tab)) = (sel, self.ui.sheet.tabs.get_mut(active))
                && tab.edit.is_none()
            {
                tab.selected = Some(sel);
            }
            self.ui.sheet.menu = Some(at);
        }
        if begin || opened {
            ctx.request_repaint();
        }
    }

    /// What a press on the tab strip asked for.
    fn sheet_tab_act(&mut self, act: TabAct, ctx: &egui::Context) {
        match act {
            TabAct::Activate(i) => {
                self.ui.sheet.activate(i);
                self.ui.sheet.focus = true;
            }
            TabAct::Close(i) => self.ui.sheet.close(i),
            TabAct::Dock(dock) => self.set_sheet_dock(dock),
            TabAct::Drawing => {
                let back = self.ui.sheet.back_to;
                self.set_sheet_dock(if back == Dock::Fill { Dock::Below } else { back });
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beside_is_shown_as_below_in_a_band_narrower_than_640_points() {
        assert_eq!(dock_for(Dock::Beside, 639.0), Dock::Below);
        assert_eq!(dock_for(Dock::Beside, 640.0), Dock::Beside);
        assert_eq!(dock_for(Dock::Below, 100.0), Dock::Below);
        assert_eq!(dock_for(Dock::Fill, 100.0), Dock::Fill, "Fill is Fill at any width");
        assert_eq!(dock_for(Dock::Beside, f32::NAN), Dock::Beside, "no number is no reason to move it");
    }

    #[test]
    fn the_sheet_opens_at_45_percent_of_the_band_with_160_points_at_least_and_the_drawing_keeping_120() {
        let (first, most) = below_sizes(800.0);
        assert_eq!(first, 360.0);
        assert!(most >= first && 800.0 - most >= DRAWING_MIN, "{first} {most}");
        // A short band: the least the sheet is.
        assert_eq!(below_sizes(300.0).0, SHEET_MIN);
        assert!(below_sizes(300.0).1 >= SHEET_MIN);
        // Never less than the least, for any input.
        for odd in [f32::NAN, f32::INFINITY, -4.0, 0.0] {
            let (first, most) = below_sizes(odd);
            assert!(first >= SHEET_MIN && most >= first && first.is_finite() && most.is_finite(), "{odd}: {first} {most}");
            let (first, most) = beside_sizes(odd);
            assert!(first >= 320.0 && most >= first && first.is_finite() && most.is_finite(), "{odd}: {first} {most}");
        }
        let (first, most) = beside_sizes(1000.0);
        assert_eq!(first, 450.0);
        assert!(1000.0 - most >= 240.0 - f32::EPSILON, "the drawing keeps room beside it: {most}");
    }
}
