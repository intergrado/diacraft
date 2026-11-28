//! The sheet's right-click menu (design 5.12): opened by a right click on a cell, on a row's number or on a section's header, or by Shift+F10.
//!
//! **Insert Row** adds a row before the selected one and, in Geometry only, **Insert Row After** adds one after it (the ribbon's Insert goes after); **Delete Row**; **Insert Section...**
//! and **Delete Section**; then the View group's two toggles, **Formulas** and **Values** (a check box each, the command `window.sheetView`). Every row is the command its ribbon control runs
//! (`window.sheetInsertRow`, `window.sheetDeleteRow`, `window.sheetInsertSection`, `window.sheetDeleteSection`), and is greyed, with the reason in its tooltip, where the selection says the
//! command would be refused (`rows.rs`). The menu closes on a press elsewhere, on Escape and on an item.

use egui::{Align2, Frame, Rect, Sense, pos2, vec2};
use serde_json::{Value, json};

use super::cards::paint_box;
use super::rows::Op;
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg};

/// The width of the menu and the height of a row, in points.
const MENU_W: f32 = 190.0;
const ROW_H: f32 = 24.0;

/// One row of the menu.
enum Row {
    /// A command: what it says, the name the window notes it under, the command and its params, and whether the selection allows it (or why not).
    Run {
        label: &'static str,
        key: &'static str,
        command: &'static str,
        params: Value,
        verdict: Result<(), String>,
    },
    /// A check box that turns one of the View group's two toggles.
    Check {
        label: &'static str,
        on: bool,
        toggle: &'static str,
    },
    Line,
}

impl DiacraftApp {
    /// The rows of the menu for the selection there is.
    fn sheet_menu_rows(&self) -> Vec<Row> {
        let (formulas, values) = (self.ui.sheet.formulas, self.ui.sheet.values);
        let run = |label, key, command, params, verdict| Row::Run { label, key, command, params, verdict };
        let insert = self.sheet_op_verdict(Op::InsertRow);
        let mut rows = vec![run("Insert Row", "Insert Row", "window.sheetInsertRow", json!({"at": "before"}), insert.clone())];
        if self.sheet_in_geometry() {
            rows.push(run("Insert Row After", "Insert Row After", "window.sheetInsertRow", json!({"at": "after"}), insert));
        }
        rows.push(run("Delete Row", "Delete Row", "window.sheetDeleteRow", json!({}), self.sheet_op_verdict(Op::DeleteRow)));
        rows.push(Row::Line);
        rows.push(run("Insert Section...", "Insert Section", "window.sheetInsertSection", json!({}), self.sheet_op_verdict(Op::InsertSection)));
        rows.push(run("Delete Section", "Delete Section", "window.sheetDeleteSection", json!({}), self.sheet_op_verdict(Op::DeleteSection)));
        rows.push(Row::Line);
        rows.push(Row::Check { label: "Formulas", on: formulas, toggle: "formulas" });
        rows.push(Row::Check { label: "Values", on: values, toggle: "values" });
        rows
    }

    /// The sheet's right-click menu, where it was opened. It closes on a press elsewhere, on Escape (`keys.rs`) and on an item.
    pub(crate) fn sheet_menu(&mut self, ctx: &egui::Context, t: &Tokens) {
        let Some(at) = self.ui.sheet.menu else { return };
        let rows = self.sheet_menu_rows();
        let mut chosen: Option<usize> = None;
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let area = egui::Area::new(egui::Id::unique("sheet_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.panel_fill).show(ui, |ui| {
                ui.set_min_width(MENU_W);
                for (i, row) in rows.iter().enumerate() {
                    match row {
                        Row::Line => {
                            ui.separator();
                        }
                        Row::Run { label, key, verdict, .. } => {
                            let (pressed, rect) = item(ui, t, label, (None, verdict.is_ok()), verdict.as_ref().err().map(String::as_str));
                            seen.push((format!("sheet.menu.{key}"), rect, verdict.is_ok()));
                            if pressed {
                                chosen = Some(i);
                            }
                        }
                        Row::Check { label, on, .. } => {
                            let (pressed, rect) = item(ui, t, label, (Some(*on), true), None);
                            seen.push((format!("sheet.menu.{label}"), rect, true));
                            if pressed {
                                chosen = Some(i);
                            }
                        }
                    }
                }
            });
        });
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        let close = chosen.is_some() || area.response.clicked_elsewhere();
        if close {
            self.ui.sheet.menu = None;
            ctx.request_repaint();
        }
        if let Some(row) = chosen.and_then(|i| rows.get(i)) {
            self.sheet_menu_run(row);
        }
    }

    /// What a row of the menu does: its command, or a toggle of the View group.
    fn sheet_menu_run(&mut self, row: &Row) {
        let result = match row {
            Row::Run { command, params, .. } => self.execute(command, params),
            Row::Check { on, toggle, .. } => self.execute("window.sheetView", &json!({ *toggle: !*on })),
            Row::Line => return,
        };
        if let Err(e) = result {
            self.set_notice(&e.to_string());
        }
    }
}

/// One row of the menu, as wide as the menu. `state` is the box's tick for a check box (`None` for a command) and whether the row can be used. → whether it was pressed, and where.
fn item(ui: &mut egui::Ui, t: &Tokens, label: &str, state: (Option<bool>, bool), why: Option<&str>) -> (bool, Rect) {
    let (ticked, enabled) = state;
    let (rect, resp) = ui.allocate_exact_size(vec2(MENU_W, ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    match ticked {
        Some(on) => widgets::name_check(&resp, enabled, on, label),
        None => widgets::name_button(&resp, enabled, label),
    }
    paint_button_bg(ui, rect, &resp, enabled, false, t, 3);
    let mut x = rect.left() + 10.0;
    if let Some(on) = ticked {
        paint_box(ui, t, rect, on, enabled);
        x = rect.left() + 30.0;
    }
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), widgets::label_colour(t, enabled));
    let pressed = enabled && resp.clicked();
    if let Some(why) = why {
        resp.on_hover_text(format!("{label} ({why})"));
    }
    (pressed, rect)
}
