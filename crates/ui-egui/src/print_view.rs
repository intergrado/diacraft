//! The Print view of the File view: the settings in a column at the left and the preview of the sheets filling the right, with a sheet
//! control and a zoom control under the preview.
//!
//! **Settings** are the printer (the system's, or Save as PDF), copies, which pages, collation, two-sided printing and colour or black and white
//! (and "Print range"), and under them what the page on view says about its paper: the paper, the way it
//! is held, the scale and the sheets it takes. Those are the page's own cells (Page Setup's), read here, not set.
//!
//! **The preview** shows one sheet of the print at a time as the printer would be given it (`print_preview.rs`): the sheet's paper, a faint line where
//! its margins are, and the part of the page that lands on it. A page that takes several sheets shows a small map of them at the corner of the
//! preview, the sheet on view marked, and a click on a sheet of the map goes to it ("shows sheet divisions", "double-clicking a tile zooms to it":
//! a click on the map is the way here). Dragging pans a sheet larger than the preview, and the zoom control sets how large the sheet is.
//!
//! **Narrow windows.** Below [`WIDE_FROM`] points of width the preview goes under the settings and both scroll as one.

use std::sync::Arc;

use diacraft_engine::print::JobPlan;
use egui::{Align2, CornerRadius, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::app::DiacraftApp;
use crate::print::{Destination, Duplex, MAX_COPIES, MAX_ZOOM, MIN_ZOOM, PageRange};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, name_button, name_check, name_toggle, paint_button_bg, tip};

/// The settings column's width when the view is wide, and the width from which it is.
const SETTINGS_W: f32 = 300.0;
pub const WIDE_FROM: f32 = 700.0;

/// The height of the strip of controls under the preview, and the room round the sheet in it.
const BAR_H: f32 = 40.0;
const PAD: f32 = 16.0;

/// The preview's height when it sits under the settings.
const STACKED_PREVIEW_H: f32 = 420.0;

/// Points to an inch at 100% zoom: what a screen calls an inch.
const POINTS_PER_INCH_AT_100: f64 = 96.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Prev,
    Next,
    Minus,
    Plus,
}

/// Draws a small control's face in `rect`.
fn paint_glyph(ui: &Ui, rect: Rect, glyph: Glyph, colour: egui::Color32) {
    let (c, ink) = (rect.center(), Stroke::new(1.6, colour));
    let p = ui.painter();
    match glyph {
        Glyph::Prev => {
            p.line_segment([c + vec2(3.0, -6.0), c + vec2(-3.0, 0.0)], ink);
            p.line_segment([c + vec2(-3.0, 0.0), c + vec2(3.0, 6.0)], ink);
        }
        Glyph::Next => {
            p.line_segment([c + vec2(-3.0, -6.0), c + vec2(3.0, 0.0)], ink);
            p.line_segment([c + vec2(3.0, 0.0), c + vec2(-3.0, 6.0)], ink);
        }
        Glyph::Minus => {
            p.line_segment([c + vec2(-6.0, 0.0), c + vec2(6.0, 0.0)], ink);
        }
        Glyph::Plus => {
            p.line_segment([c + vec2(-6.0, 0.0), c + vec2(6.0, 0.0)], ink);
            p.line_segment([c + vec2(0.0, -6.0), c + vec2(0.0, 6.0)], ink);
        }
    }
}

impl DiacraftApp {
    /// A small square control with a drawn face. Noted as `name`.
    fn glyph_button(&mut self, ui: &mut Ui, t: &Tokens, rect: Rect, (name, label): (&str, &str), glyph: Glyph, enabled: bool) -> bool {
        let resp = ui.interact(rect, ui.scope_id().with(name), if enabled { Sense::click() } else { Sense::hover() });
        name_button(&resp, enabled, label);
        paint_button_bg(ui, rect, &resp, enabled, false, t, 4);
        paint_glyph(ui, rect, glyph, if enabled { t.icon_ink } else { t.icon_ink_disabled });
        self.note(name, rect, enabled);
        resp.on_hover_text(tip(label, enabled)).clicked() && enabled
    }

    /// A row of buttons, one pressed. Each is noted as `name.key`. → the key pressed.
    fn segmented(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        name: &str,
        options: &[(&'static str, &str)],
        current: &str,
        enabled: bool,
    ) -> Option<&'static str> {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width().max(60.0), 28.0), Sense::hover());
        let each = row.width() / options.len().max(1) as f32;
        let mut chosen = None;
        for (i, (key, label)) in options.iter().enumerate() {
            let r = Rect::from_min_size(row.min + vec2(each * i as f32, 0.0), vec2(each - 2.0, row.height()));
            let resp = ui.interact(r, ui.scope_id().with((name, *key)), if enabled { Sense::click() } else { Sense::hover() });
            let selected = *key == current;
            name_toggle(&resp, enabled, selected, label);
            ui.painter().rect_filled(r, CornerRadius::same(4), t.input_fill);
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            paint_button_bg(ui, r, &resp, enabled, selected, t, 4);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, theme::font(size::LABEL), widgets::label_colour(t, enabled));
            self.note(format!("{name}.{key}"), r, enabled);
            if resp.on_hover_text(tip(label, enabled)).clicked() && enabled {
                chosen = Some(*key);
            }
        }
        chosen
    }

    /// The Print page: settings and preview.
    pub(crate) fn print_page(&mut self, ui: &mut Ui, t: &Tokens, body: Rect) {
        let ctx = ui.ctx().clone();
        self.print_list_printers(&ctx);
        self.ui.print.preview.poll(&ctx);
        ui.painter().text(body.left_top(), Align2::LEFT_TOP, "Print", theme::font(26.0), t.text);
        let content = Rect::from_min_max(body.left_top() + vec2(0.0, 48.0), body.right_bottom());
        if self.session().document().is_none() {
            ui.painter().text(
                content.left_top() + vec2(0.0, 8.0),
                Align2::LEFT_TOP,
                "There is no drawing to print.",
                theme::font(size::TAB + 1.0),
                t.text_disabled,
            );
            return;
        }
        let plan = self.print_plan();
        if content.width() >= WIDE_FROM {
            let settings = Rect::from_min_size(content.min, vec2(SETTINGS_W, content.height()));
            let preview = Rect::from_min_max(pos2(settings.right() + 28.0, content.top()), pos2(content.right(), content.bottom() - 8.0));
            ui.scope_builder(UiBuilder::new().max_rect(settings), |ui| {
                egui::ScrollArea::vertical().id_salt("print_settings").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(SETTINGS_W - 12.0);
                    self.print_settings(ui, t, &plan);
                });
            });
            self.print_preview_pane(ui, t, preview, &plan);
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                egui::ScrollArea::vertical().id_salt("print_stacked").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width((content.width() - 12.0).max(120.0));
                    self.print_settings(ui, t, &plan);
                    ui.add_space(12.0);
                    let (preview, _) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), STACKED_PREVIEW_H), Sense::hover());
                    self.print_preview_pane(ui, t, preview, &plan);
                    ui.add_space(8.0);
                });
            });
        }
    }

    fn print_settings(&mut self, ui: &mut Ui, t: &Tokens, plan: &Result<Arc<JobPlan>, String>) {
        let ctx = ui.ctx().clone();
        let to_pdf = self.ui.print.choices.destination == Some(Destination::Pdf);
        let running = self.printing_now();
        // Print, or Cancel while it goes.
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(124.0, 48.0), Sense::hover());
            if running {
                let resp = ui.interact(r, ui.scope_id().with("print_cancel"), Sense::click());
                name_button(&resp, true, "Cancel");
                paint_button_bg(ui, r, &resp, true, false, t, 6);
                ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "Cancel", theme::font(size::TAB + 1.0), t.text);
                self.note("print.cancel", r, true);
                if resp.on_hover_text("Stop the print before it reaches the printer").clicked() {
                    self.cancel_print();
                }
            } else {
                let enabled = self.can_print();
                let label = if to_pdf { "Save as PDF" } else { "Print" };
                let resp = ui.interact(r, ui.scope_id().with("print_start"), if enabled { Sense::click() } else { Sense::hover() });
                name_button(&resp, enabled, label);
                let fill = match (enabled, resp.hovered()) {
                    (false, _) => t.panel_header_fill,
                    (true, true) => t.accent_hover,
                    (true, false) => t.accent,
                };
                ui.painter().rect_filled(r, CornerRadius::same(6), fill);
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    label,
                    theme::font(size::TAB + 2.0),
                    if enabled { t.accent_text } else { t.text_disabled },
                );
                self.note("print.start", r, enabled);
                if resp.on_hover_text(tip(label, enabled)).clicked() && enabled {
                    self.begin_print(&ctx, None);
                }
            }
            ui.add_space(8.0);
            // Copies: less, the number, more.
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Copies").font(theme::font(size::LABEL)).color(t.text_dim));
                let copies = self.ui.print.choices.copies;
                let (row, _) = ui.allocate_exact_size(vec2(96.0, 26.0), Sense::hover());
                let editable = !to_pdf && !running;
                let (down, up) =
                    (Rect::from_min_size(row.min, vec2(26.0, 26.0)), Rect::from_min_size(row.right_top() - vec2(26.0, 0.0), vec2(26.0, 26.0)));
                if self.glyph_button(ui, t, down, ("print.copies.down", "Fewer copies"), Glyph::Minus, editable && copies > 1) {
                    self.ui.print.choices.copies = copies - 1;
                }
                if self.glyph_button(ui, t, up, ("print.copies.up", "More copies"), Glyph::Plus, editable && copies < MAX_COPIES) {
                    self.ui.print.choices.copies = copies + 1;
                }
                let colour = widgets::label_colour(t, editable);
                ui.painter().text(row.center(), Align2::CENTER_CENTER, copies.to_string(), theme::font(size::TAB + 1.0), colour);
                self.note("print.copies", row, editable);
            });
        });
        ui.add_space(12.0);

        // The printer.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Printer").font(theme::font(size::TAB)).color(t.text));
            if self.ui.print.service.is_some() {
                let (r, _) = ui.allocate_exact_size(vec2(64.0, 20.0), Sense::hover());
                let resp = ui.interact(r, ui.scope_id().with("print_refresh"), Sense::click());
                name_button(&resp, !self.ui.print.printers_loading(), "Refresh");
                let enabled = !self.ui.print.printers_loading() && !running;
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "Refresh",
                    theme::font(size::LABEL),
                    if enabled { t.accent } else { t.text_disabled },
                );
                self.note("print.refresh", r, enabled);
                if resp.on_hover_text("List the printers again").clicked() && enabled {
                    self.ui.print.forget_printers();
                }
            }
        });
        self.printer_choice(ui, t, running);
        ui.add_space(14.0);

        // The settings.
        ui.label(egui::RichText::new("Settings").font(theme::font(size::TAB)).color(t.text));
        ui.add_space(4.0);
        let range = self.ui.print.choices.range;
        let pages = [("all", "All pages"), ("current", "Current page")];
        if let Some(key) = self.segmented(ui, t, "print.pages", &pages, range.word(), !running) {
            self.ui.print.choices.range = if key == "current" { PageRange::Current } else { PageRange::All };
            self.ui.print.sheet = 0;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        ui.add_space(8.0);
        let for_printer = matches!(self.ui.print.choices.destination, Some(Destination::Printer(_))) && !running;
        let mut collate = self.ui.print.choices.collate;
        let can_collate = for_printer && self.ui.print.choices.copies > 1;
        let check = ui.add_enabled(can_collate, egui::Checkbox::new(&mut collate, egui::RichText::new("Collated").font(theme::font(size::LABEL))));
        name_check(&check, can_collate, collate, "Collated");
        self.note("print.collate", check.rect, can_collate);
        if check
            .on_hover_text(if can_collate { "Copies come out complete one after the other" } else { "Only for several copies sent to a printer" })
            .changed()
        {
            self.ui.print.choices.collate = collate;
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Two-sided printing").font(theme::font(size::LABEL)).color(t.text_dim));
        let mut duplex = self.ui.print.choices.duplex;
        let mut items: Vec<(Duplex, Rect)> = Vec::new();
        let width = (ui.available_width() - 4.0).max(60.0);
        let combo = ui
            .add_enabled_ui(for_printer, |ui| {
                egui::ComboBox::from_id_salt("print_duplex")
                    .selected_text(egui::RichText::new(duplex.label()).font(theme::font(size::LABEL)))
                    .width(width)
                    .show_ui(ui, |ui| {
                        for d in Duplex::ALL {
                            let r = ui.selectable_value(&mut duplex, d, egui::RichText::new(d.label()).font(theme::font(size::LABEL)));
                            items.push((d, r.rect));
                        }
                    })
            })
            .inner;
        let combo_enabled = for_printer;
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, combo_enabled, "Two-sided printing"));
        self.note("print.duplex", combo.response.rect, combo_enabled);
        for (d, r) in items {
            self.note(format!("print.duplex.{}", d.word()), r, combo_enabled);
        }
        if for_printer && duplex != self.ui.print.choices.duplex {
            self.ui.print.choices.duplex = duplex;
        }
        ui.add_space(8.0);
        // Colour or black and white: for a printer and for Save as PDF alike (a PDF is what a printer would be given), and the preview follows it.
        let colour = self.ui.print.choices.color().word();
        let colours = [("color", "Color"), ("grayscale", "Black and white")];
        if let Some(key) = self.segmented(ui, t, "print.color", &colours, colour, !running) {
            self.ui.print.choices.grayscale = key == "grayscale";
        }
        ui.add_space(16.0);

        // What the page says of its paper.
        ui.label(egui::RichText::new("Paper").font(theme::font(size::TAB)).color(t.text));
        ui.add_space(2.0);
        match plan {
            Ok(plan) => {
                for line in summary(plan, self.ui.print.sheet) {
                    ui.add(egui::Label::new(egui::RichText::new(line).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
                }
            }
            Err(why) => {
                ui.add(egui::Label::new(egui::RichText::new(why).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
            }
        }
        ui.add_space(2.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new("Paper, orientation, margins and scale are the page's own: Page Setup changes them.")
                    .font(theme::font(size::GROUP))
                    .color(t.text_disabled),
            )
            .wrap(),
        );
        // The page's own paper, orientation and scale are changed in the Page Setup dialog, which opens over this view.
        ui.add_space(4.0);
        let can_set_up = self.command_enabled("window.pageSetup").is_ok();
        let set_up = ui.add_enabled(
            can_set_up,
            egui::Button::new(egui::RichText::new("Page Setup...").font(theme::font(size::LABEL))).min_size(egui::vec2(110.0, 24.0)),
        );
        self.note("print.page_setup", set_up.rect, can_set_up);
        if set_up.clicked()
            && let Err(e) = self.execute("window.pageSetup", &serde_json::json!({"tab": "Print Setup"}))
        {
            self.set_notice(&e.to_string());
        }
        if let Some(outcome) = self.ui.print.outcome.clone() {
            ui.add_space(10.0);
            ui.add(egui::Label::new(egui::RichText::new(outcome).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
        }
    }

    /// The printer box and what stands in for it when there is no printer.
    fn printer_choice(&mut self, ui: &mut Ui, t: &Tokens, running: bool) {
        let loading = self.ui.print.printers_loading();
        let list: Vec<(String, String)> =
            self.ui.print.printer_list().map(|l| l.iter().map(|p| (p.name.clone(), p.shown().to_owned())).collect()).unwrap_or_default();
        let current = self.ui.print.choices.destination.clone();
        let current_text = match &current {
            Some(Destination::Printer(name)) => list.iter().find(|(n, _)| n == name).map_or_else(|| name.clone(), |(_, label)| label.clone()),
            Some(Destination::Pdf) => "Save as PDF".to_owned(),
            None if loading => "Finding printers...".to_owned(),
            None => "Choose a printer".to_owned(),
        };
        let mut next = current.clone();
        let mut items: Vec<(String, Rect)> = Vec::new();
        let enabled = !running && !loading;
        let combo = egui::ComboBox::from_id_salt("print_printer")
            .selected_text(egui::RichText::new(shown_short(&current_text)).font(theme::font(size::LABEL)))
            .width((ui.available_width() - 4.0).max(60.0))
            .show_ui(ui, |ui| {
                for (name, label) in &list {
                    let r = ui.selectable_value(
                        &mut next,
                        Some(Destination::Printer(name.clone())),
                        egui::RichText::new(shown_short(label)).font(theme::font(size::LABEL)),
                    );
                    items.push((name.clone(), r.rect));
                }
                let r = ui.selectable_value(&mut next, Some(Destination::Pdf), egui::RichText::new("Save as PDF").font(theme::font(size::LABEL)));
                items.push((crate::print::PDF_WORD.to_owned(), r.rect));
            });
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, "Printer"));
        self.note("print.printer", combo.response.rect, enabled);
        for (name, r) in items {
            self.note(format!("print.printer.{name}"), r, enabled);
        }
        if enabled && next != current {
            self.ui.print.choices.destination = next;
        }
        let words = if loading {
            "Finding printers...".to_owned()
        } else if self.ui.print.service.is_none() && self.has_file_host() {
            // The browser: a page cannot reach a printer, and a PDF is what a printer would be given: it is downloaded, and printed from where the person opens it.
            "A web page cannot reach your printer. Save as PDF downloads what a printer would be given: print that file from your PDF viewer or your browser.".to_owned()
        } else if self.ui.print.service.is_none() {
            "This window has no printers. Save as PDF writes what a printer would be given.".to_owned()
        } else if list.is_empty() {
            "No printer was found. Save as PDF writes what a printer would be given.".to_owned()
        } else {
            String::new()
        };
        if !words.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(words).font(theme::font(size::GROUP)).color(t.text_dim)).wrap());
        }
    }

    /// The preview: the sheet, its map, and the controls under it.
    fn print_preview_pane(&mut self, ui: &mut Ui, t: &Tokens, pane: Rect, plan: &Result<Arc<JobPlan>, String>) {
        let ctx = ui.ctx().clone();
        let area = Rect::from_min_max(pane.min, pos2(pane.right(), pane.bottom() - BAR_H));
        ui.painter().rect_filled(pane, CornerRadius::same(4), t.canvas_fill);
        self.note("print.preview", area, true);
        let plan = match plan {
            Ok(plan) => Arc::clone(plan),
            Err(why) => {
                let text = egui::RichText::new(why).font(theme::font(size::TAB)).color(t.notice_text);
                let galley = ui.painter().layout(why.clone(), theme::font(size::TAB), t.notice_text, (area.width() - 2.0 * PAD).max(40.0));
                ui.painter().galley(area.center() - galley.size() / 2.0, galley, t.notice_text);
                let _ = text;
                return;
            }
        };
        let count = plan.sheet_count();
        if self.ui.print.sheet >= count {
            self.ui.print.sheet = count.saturating_sub(1);
        }
        let n = self.ui.print.sheet;
        let Some((page_plan, sheet)) = plan.sheet(n) else { return };
        let (sheet, page_plan) = (sheet.clone(), page_plan.clone());

        // Where the sheet is: the whole of it in the area, or at the zoom the person set, moved by what they dragged.
        // Beside a map of the sheets there is room for it, where the preview is wide enough to spare it: the sheet fits what is left.
        let gutter = if page_plan.sheets.len() > 1 && area.width() >= 560.0 { map_size(&page_plan).x + 24.0 } else { 0.0 };
        let room = Rect::from_min_max(area.min + vec2(PAD, PAD), area.max - vec2(PAD + gutter, PAD));
        let fit = f64::from(room.width().max(20.0) / sheet.width as f32).min(f64::from(room.height().max(20.0) / sheet.height as f32));
        let points_per_inch = self.ui.print.zoom.map_or(fit, |z| z / 100.0 * POINTS_PER_INCH_AT_100);
        let (w, h) = (sheet.width as f32 * points_per_inch as f32, sheet.height as f32 * points_per_inch as f32);
        let slack = vec2(((w - room.width()) / 2.0).max(0.0), ((h - room.height()) / 2.0).max(0.0));
        let drag = ui.interact(area, ui.scope_id().with("print_drag"), Sense::drag());
        if drag.dragged() {
            self.ui.print.pan += drag.drag_delta();
        }
        self.ui.print.pan = vec2(self.ui.print.pan.x.clamp(-slack.x, slack.x), self.ui.print.pan.y.clamp(-slack.y, slack.y));
        let at = room.center() + self.ui.print.pan;
        let sheet_rect = Rect::from_center_size(at, vec2(w, h));
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(sheet_rect.translate(vec2(3.0, 4.0)), CornerRadius::ZERO, t.page_shadow);
        painter.rect(sheet_rect, CornerRadius::ZERO, t.page_fill, Stroke::new(1.0, t.page_border), StrokeKind::Inside);
        // Where the margins are.
        let to_screen =
            |x: f64, y: f64| pos2(sheet_rect.left() + x as f32 * points_per_inch as f32, sheet_rect.top() + y as f32 * points_per_inch as f32);
        let printable = Rect::from_min_max(to_screen(sheet.printable.x0, sheet.printable.y0), to_screen(sheet.printable.x1, sheet.printable.y1));
        // (A margin of a hundredth of a point is no margin: the sheet's own edge is drawn already.)
        if (printable.min - sheet_rect.min).length() > 0.5 || (printable.max - sheet_rect.max).length() > 0.5 {
            painter.add(Shape::dashed_line(
                &[printable.left_top(), printable.right_top(), printable.right_bottom(), printable.left_bottom(), printable.left_top()],
                Stroke::new(1.0, t.grid_major),
                4.0,
                3.0,
            ));
        }
        // The picture of the sheet, at a resolution that serves its size on the screen.
        let generation = self.ui.pictures.generation();
        let revision = self.session().revision();
        let max_side = f64::from(ctx.input(|i| i.max_texture_side) as f32);
        let ppi = crate::print_preview::resolution(f64::from(w), f64::from(ctx.pixels_per_point()), (sheet.width, sheet.height), max_side);
        if let Some(doc) = self.session().document().cloned() {
            let color = self.ui.print.choices.color();
            self.ui.print.preview.want(&ctx, &doc, (generation, revision), n, &sheet, (ppi, color));
        }
        if let Some(shown) = self.ui.print.preview.shown_for(generation, n) {
            painter.image(shown.texture.id(), sheet_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), t.picture_tint);
        } else if self.ui.print.preview.busy() {
            painter.text(sheet_rect.center(), Align2::CENTER_CENTER, "Drawing the sheet...", theme::font(size::LABEL), t.text_dim);
        }
        if let Some(lost) = self.ui.print.preview.lost() {
            let galley = painter.layout(lost.to_owned(), theme::font(size::LABEL), t.notice_text, (area.width() - 2.0 * PAD).max(40.0));
            painter.galley(pos2(area.center().x - galley.size().x / 2.0, area.bottom() - galley.size().y - 6.0), galley, t.notice_text);
        }
        self.note("print.sheet", sheet_rect, true);

        // The map of the sheets of the page, when it takes more than one.
        if page_plan.sheets.len() > 1 {
            self.sheet_map(ui, t, area, &plan, n);
        }

        // Under the preview.
        let bar = Rect::from_min_max(pos2(pane.left(), pane.bottom() - BAR_H), pane.right_bottom());
        self.preview_bar(ui, t, bar, (n, count), fit, &page_plan.name);

        // The keys that turn the sheets, when no field has the keyboard.
        if !ctx.text_edit_focused() && count > 1 {
            let (prev, next) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                )
            });
            if prev {
                self.go_to_sheet(n.saturating_sub(1));
            } else if next {
                self.go_to_sheet((n + 1).min(count - 1));
            }
        }
    }

    pub(crate) fn go_to_sheet(&mut self, n: usize) {
        if self.ui.print.sheet != n {
            self.ui.print.sheet = n;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
    }

    /// The tiles of the page on view as small squares at the corner of the preview: the sheet on view marked, and a click goes to a sheet.
    fn sheet_map(&mut self, ui: &mut Ui, t: &Tokens, area: Rect, plan: &JobPlan, current: usize) {
        let Some((page_plan, _)) = plan.sheet(current) else { return };
        let cell = map_cell(page_plan);
        let size = map_size(page_plan);
        let origin = pos2(area.right() - size.x - 14.0, area.top() + 14.0);
        let frame = Rect::from_min_size(origin, size).expand(5.0);
        ui.painter().rect(frame, CornerRadius::same(4), t.panel_fill, Stroke::new(1.0, t.separator), StrokeKind::Inside);
        let first = page_plan.sheets.first().map_or(0, |s| s.number);
        for sheet in &page_plan.sheets {
            let r = Rect::from_min_size(origin + vec2(cell * sheet.col as f32, cell * sheet.row as f32), vec2(cell - 2.0, cell - 2.0));
            let resp = ui.interact(r, ui.scope_id().with(("print_map", sheet.number)), Sense::click());
            let selected = sheet.number == current;
            name_toggle(&resp, true, selected, &format!("Sheet {}", sheet.number + 1));
            let fill = if selected {
                t.accent
            } else if resp.hovered() {
                t.hover
            } else {
                t.page_fill
            };
            ui.painter().rect(r, CornerRadius::same(2), fill, Stroke::new(1.0, if selected { t.accent } else { t.page_border }), StrokeKind::Inside);
            self.note(format!("print.map.{}", sheet.number + 1), r, true);
            if resp.on_hover_text(format!("Sheet {} (column {}, row {})", sheet.number + 1 - first + 1, sheet.col + 1, sheet.row + 1)).clicked() {
                self.go_to_sheet(sheet.number);
            }
        }
    }

    /// The strip under the preview: the sheet control at the left and the zoom control at the right.
    fn preview_bar(&mut self, ui: &mut Ui, t: &Tokens, bar: Rect, (n, count): (usize, usize), fit: f64, page_name: &str) {
        let y = bar.center().y;
        let square = |x: f32| Rect::from_center_size(pos2(x, y), vec2(28.0, 28.0));
        let mut x = bar.left() + 16.0;
        if self.glyph_button(ui, t, square(x), ("print.prev", "Previous sheet"), Glyph::Prev, n > 0) {
            self.go_to_sheet(n - 1);
        }
        x += 36.0;
        let label = format!("{} of {}", n + 1, count);
        let font = theme::font(size::LABEL + 1.0);
        let width = widgets::text_width(ui, &label, &font);
        ui.painter().text(pos2(x - 14.0, y), Align2::LEFT_CENTER, &label, font, t.text);
        self.note("print.sheetnumber", Rect::from_min_size(pos2(x - 14.0, y - 10.0), vec2(width, 20.0)), true);
        x += width + 8.0;
        if self.glyph_button(ui, t, square(x), ("print.next", "Next sheet"), Glyph::Next, n + 1 < count) {
            self.go_to_sheet(n + 1);
        }
        // Which page the sheet is of, when there is room for it.
        let free_from = x + 26.0;
        let zoom_w = 250.0;
        let room = bar.right() - zoom_w - free_from;
        if room > 60.0 {
            let font = theme::font(size::GROUP);
            let clipped = ui.painter().with_clip_rect(Rect::from_min_size(pos2(free_from, bar.top()), vec2(room, bar.height())));
            clipped.text(pos2(free_from, y), Align2::LEFT_CENTER, shown_short(page_name), font, t.text_dim);
        }
        // Zoom: smaller, how much, larger, and the whole sheet.
        let percent = self.ui.print.zoom.unwrap_or(fit / POINTS_PER_INCH_AT_100 * 100.0);
        let mut zx = bar.right() - zoom_w + 16.0;
        if self.glyph_button(ui, t, square(zx), ("print.zoomOut", "Zoom out"), Glyph::Minus, percent > MIN_ZOOM + 0.5) {
            self.ui.print.zoom = Some((percent / 1.25).clamp(MIN_ZOOM, MAX_ZOOM));
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        zx += 36.0;
        let text = format!("{}%", percent.round() as i64);
        ui.painter().text(pos2(zx + 14.0, y), Align2::CENTER_CENTER, &text, theme::font(size::LABEL + 1.0), t.text);
        self.note("print.zoomPercent", Rect::from_center_size(pos2(zx + 14.0, y), vec2(48.0, 20.0)), true);
        zx += 42.0;
        if self.glyph_button(ui, t, square(zx), ("print.zoomIn", "Zoom in"), Glyph::Plus, percent < MAX_ZOOM - 0.5) {
            self.ui.print.zoom = Some((percent * 1.25).clamp(MIN_ZOOM, MAX_ZOOM));
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        zx += 46.0;
        let fit_rect = Rect::from_center_size(pos2(zx + 10.0, y), vec2(56.0, 28.0));
        let resp: Response = ui.interact(fit_rect, ui.scope_id().with("print_fit"), Sense::click());
        name_toggle(&resp, true, self.ui.print.zoom.is_none(), "Fit the sheet");
        paint_button_bg(ui, fit_rect, &resp, true, self.ui.print.zoom.is_none(), t, 4);
        ui.painter().text(fit_rect.center(), Align2::CENTER_CENTER, "Fit", theme::font(size::LABEL), t.text);
        self.note("print.fit", fit_rect, true);
        if resp.on_hover_text("Show the whole sheet").clicked() {
            self.ui.print.zoom = None;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
    }
}

/// How large a tile of the map of a page's sheets is, in points: the whole map is about 96 points on its longer side, and each tile 10 to 26.
fn map_cell(page: &diacraft_engine::print::PagePlan) -> f32 {
    (96.0 / page.across.max(page.down).max(1) as f32).clamp(10.0, 26.0)
}

/// The size of the map of a page's sheets.
fn map_size(page: &diacraft_engine::print::PagePlan) -> egui::Vec2 {
    let cell = map_cell(page);
    vec2(cell * page.across.max(1) as f32, cell * page.down.max(1) as f32)
}

/// A name cut short for a place with little room.
fn shown_short(text: &str) -> String {
    diacraft_engine::shown(text, 48)
}

/// What the page on view says of its paper, in lines: the paper and the way it is held, the scale, and the sheets. And, for a print of several
/// pages, how many pages and sheets in all.
pub(crate) fn summary(plan: &JobPlan, sheet: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if plan.pages.len() > 1 {
        lines.push(format!("{} pages, {} sheets in all.", plan.pages.len(), plan.sheet_count()));
    }
    let Some((page, _)) = plan.sheet(sheet) else { return lines };
    let inches = |n: f64| {
        let text = format!("{n:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    let paper = if page.paper_known { page.paper.name.to_owned() } else { format!("{} (the paper is not one DiaCraft knows)", page.paper.name) };
    lines.push(format!(
        "{}: {paper}, {}, {} by {} inches.",
        page.name,
        page.orientation.name(),
        inches(page.sheet_size.0),
        inches(page.sheet_size.1)
    ));
    let percent = page.scale * 100.0;
    if (percent - 100.0).abs() < 0.05 {
        lines.push("Prints at 100%.".to_owned());
    } else {
        lines.push(format!("Prints at {}%.", inches(percent)));
    }
    let n = page.sheets.len();
    lines.push(if n == 1 { "Takes 1 sheet.".to_owned() } else { format!("Takes {n} sheets: {} across and {} down.", page.across, page.down) });
    lines
}

#[cfg(test)]
mod tests {
    use diacraft_engine::print::plan;
    use diacraft_testkit::build::{DocBuilder, PageBuilder};

    use super::*;

    fn doc() -> diacraft_engine::doc::Document {
        let page = |w: &str, h: &str| PageBuilder::new().cell("PageWidth", w).cell("PageHeight", h).cell("PageScale", "1").cell("DrawingScale", "1");
        DocBuilder::new()
            .page(
                1,
                page("11", "8.5").cell("PageLeftMargin", "0").cell("PageRightMargin", "0").cell("PageTopMargin", "0").cell("PageBottomMargin", "0"),
            )
            .page(2, page("17", "11").cell("PrintPageOrientation", "1").cell("PaperKind", "9"))
            .build()
    }

    #[test]
    fn the_summary_says_the_paper_the_scale_and_the_sheets() {
        let doc = doc();
        let one = plan(&doc, diacraft_engine::print::Pages::One(1)).unwrap();
        assert_eq!(summary(&one, 0), ["Page-1: Letter, landscape, 11 by 8.5 inches.", "Prints at 100%.", "Takes 1 sheet."].map(str::to_owned));
        let both = plan(&doc, diacraft_engine::print::Pages::All).unwrap();
        let lines = summary(&both, 1);
        assert_eq!(lines[0], "2 pages, 4 sheets in all.");
        assert!(lines[1].starts_with("Page-2: A4, portrait, 8.27 by 11.69 inches."), "{lines:?}");
        assert_eq!(lines[3], "Takes 3 sheets: 3 across and 1 down.");
        assert!(summary(&both, 9).len() == 1, "a sheet that is not there names no page");
    }
}
