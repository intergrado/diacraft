//! The File view (the backstage): a narrow rail with Home, New and Open at the top and Account and Options at the bottom, each
//! with its label under its icon, a Back button above them, and the open page to the right. Print stands
//! after Save As and opens the Print page (`print_view.rs`).
//!
//! **About** stands under Account: the specification puts the About box on the Account page, a page this window does not have yet,
//! so the entry is in the rail itself, and runs `app.about` (the octopus cover with the version, the git release, the year and the licence; `splash.rs`).
//!
//! What works: Back, Home, New (its gallery of templates is `new_page.rs`) and the Blank drawing tile, which makes a new drawing (`file.new`), and Open, which shows the
//! native file dialog and opens what is chosen (`file_open.rs`; disabled where the app has no dialog, as on the web). Options opens the Options
//! dialog (`options_dialog.rs`), where the Developer tab is turned on. Account does nothing yet. They are all drawn, and the ones that do nothing are quiet.
//! The gallery headings of the New page are the ones R24 names, apart from Office's and the organization's.
//!
//! The rail and the tile gallery follow the sibling craft apps' File views (`docs/provenance.md`); the code is written for
//! this window.

use egui::{Align2, CornerRadius, Frame, Key, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::app::DiacraftApp;
use crate::icons::{self, Badge, Base, Icon};
use crate::state::BackstagePage;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, tip};

/// The rail's width.
const RAIL_W: f32 = 92.0;

/// One entry of the rail's height, and the least it is made when the rail is short.
const ENTRY_H: f32 = 70.0;
const MIN_ENTRY_H: f32 = 52.0;

/// The name of the tile that makes an empty drawing.
pub const BLANK_TILE: &str = "Blank drawing";

/// What an entry of the rail does when it is not a page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Nothing,
    /// Shows the Open dialog (`file.open`), which is not a page: the File view closes when a file is chosen.
    Open,
    /// `file.save`, or the Save dialog for a drawing with no file yet.
    Save,
    /// The Save dialog and `file.save_as`.
    SaveAs,
    /// The Export dialog and `file.export`: the page on view as a PNG, an SVG or a PDF.
    Export,
    /// The Options dialog (`window.options`), where the Developer tab is turned on: the File view closes behind it.
    Options,
    /// `app.about`: the About panel over the File view, which stays.
    About,
}

/// An entry of the rail.
#[derive(Clone, Copy)]
struct Entry {
    label: &'static str,
    icon: Icon,
    /// The page it opens; `None`: it opens none.
    page: Option<BackstagePage>,
    act: Act,
}

const TOP: [Entry; 7] = [
    Entry { label: "Home", icon: Icon::of(Base::Page), page: Some(BackstagePage::Home), act: Act::Nothing },
    Entry { label: "New", icon: Icon::of(Base::Page).with(Badge::Plus), page: Some(BackstagePage::New), act: Act::Nothing },
    Entry { label: "Open", icon: Icon::of(Base::FolderOpen), page: None, act: Act::Open },
    Entry { label: "Save", icon: Icon::of(Base::Save), page: None, act: Act::Save },
    Entry { label: "Save As", icon: Icon::of(Base::Save).with(Badge::Plus), page: None, act: Act::SaveAs },
    Entry { label: "Print", icon: Icon::of(Base::Printer), page: Some(BackstagePage::Print), act: Act::Nothing },
    Entry { label: "Export", icon: Icon::of(Base::Picture).with(Badge::Arrow), page: None, act: Act::Export },
];

const BOTTOM: [Entry; 3] = [
    Entry { label: "Account", icon: Icon::of(Base::Person), page: None, act: Act::Nothing },
    Entry { label: "About", icon: Icon::of(Base::Help), page: None, act: Act::About },
    Entry { label: "Options", icon: Icon::of(Base::Gear), page: None, act: Act::Options },
];

impl DiacraftApp {
    pub(crate) fn backstage(&mut self, ui: &mut Ui, t: Tokens, open: BackstagePage) {
        let frame = Frame::new().fill(t.backstage_fill);
        // Escape closes an open drop-down box and not the File view under it: whether one is open is asked before the page draws, which closes it.
        let box_open = egui::Popup::is_any_open(ui.ctx());
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let area = ui.max_rect();
            let rail = Rect::from_min_size(area.min, vec2(RAIL_W, area.height()));
            ui.painter().rect_filled(rail, 0.0, t.rail_fill);
            self.rail(ui, &t, rail, open);
            let body = Rect::from_min_max(pos2(rail.right() + 32.0, area.top() + 28.0), pos2(area.right() - 24.0, area.bottom()));
            match open {
                BackstagePage::Home => self.home_page(ui, &t, body),
                BackstagePage::New => self.new_page(ui, &t, body),
                BackstagePage::Print => self.print_page(ui, &t, body),
            }
            if !box_open && ui.input(|i| i.key_pressed(Key::Escape)) {
                // The detail of a template is the innermost thing on the New page: Escape closes it before it leaves the File view.
                if open == BackstagePage::New && self.ui.new_page.detail.take().is_some() {
                    return;
                }
                self.ui.backstage = None;
            }
        });
    }

    fn rail(&mut self, ui: &mut Ui, t: &Tokens, rail: Rect, open: BackstagePage) {
        // The Back button.
        let back = Rect::from_center_size(pos2(rail.center().x, rail.top() + 34.0), vec2(36.0, 36.0));
        let resp = ui.interact(back, ui.scope_id().with("backstage_back"), Sense::click());
        widgets::name_button(&resp, true, "Back");
        if resp.hovered() {
            ui.painter().circle_filled(back.center(), 18.0, t.rail_hover);
        }
        ui.painter().circle_stroke(back.center(), 17.0, Stroke::new(1.5, t.rail_text));
        let (c, ink) = (back.center(), Stroke::new(1.8, t.rail_text));
        ui.painter().line_segment([c + vec2(7.0, 0.0), c + vec2(-7.0, 0.0)], ink);
        ui.painter().line_segment([c + vec2(-7.0, 0.0), c + vec2(-1.5, -5.5)], ink);
        ui.painter().line_segment([c + vec2(-7.0, 0.0), c + vec2(-1.5, 5.5)], ink);
        self.note("backstage.back", back, true);
        if resp.on_hover_text("Back").clicked() {
            self.ui.backstage = None;
        }
        // The entries share the rail's height: the least keeps an icon over its label. A rail too short even for that scrolls, with the
        // bottom entries following the top ones, so that no label lies over another.
        let h = ((rail.height() - 88.0) / (TOP.len() + BOTTOM.len()) as f32).clamp(MIN_ENTRY_H, ENTRY_H);
        if rail.height() - 88.0 >= h * (TOP.len() + BOTTOM.len()) as f32 {
            let mut y = rail.top() + 80.0;
            for entry in TOP {
                self.rail_entry(ui, t, Rect::from_min_size(pos2(rail.left(), y), vec2(RAIL_W, h)), entry, open);
                y += h;
            }
            let mut y = rail.bottom() - 8.0 - h * BOTTOM.len() as f32;
            for entry in BOTTOM {
                self.rail_entry(ui, t, Rect::from_min_size(pos2(rail.left(), y), vec2(RAIL_W, h)), entry, open);
                y += h;
            }
            return;
        }
        let list = Rect::from_min_max(pos2(rail.left(), rail.top() + 80.0), rail.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("rail_scroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    for entry in TOP.into_iter().chain(BOTTOM) {
                        let (r, _) = ui.allocate_exact_size(vec2(RAIL_W, h), Sense::hover());
                        self.rail_entry(ui, t, r, entry, open);
                    }
                });
        });
    }

    fn rail_entry(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, entry: Entry, open: BackstagePage) {
        // Open works when the app has a file dialog and nothing is being opened.
        let enabled = entry.page.is_some_and(|page| page != BackstagePage::Print || self.can_open_print())
            || match entry.act {
                Act::Open => self.can_pick_file(),
                Act::Save => self.can_save(),
                Act::SaveAs => self.can_save_as(),
                Act::Export => self.can_export(),
                // The dialog works on a window setting and needs no drawing.
                Act::Options => true,
                // The panel needs nothing: no drawing, no dialog.
                Act::About => true,
                Act::Nothing => false,
            };
        let active = entry.page == Some(open);
        let resp = ui.interact(r, ui.scope_id().with(("rail", entry.label)), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, entry.label);
        let painter = ui.painter();
        if active {
            painter.rect_filled(r.shrink2(vec2(6.0, 3.0)), CornerRadius::same(6), t.rail_active);
        } else if enabled && resp.hovered() {
            painter.rect_filled(r.shrink2(vec2(6.0, 3.0)), CornerRadius::same(6), t.rail_hover);
        }
        // On the rail an icon is drawn in the rail's text colour, whatever the tokens say for an icon on a panel; one that does
        // nothing is drawn fainter.
        let ink = Tokens {
            icon_ink: t.rail_text,
            icon_accent: t.rail_text,
            icon_fill: t.rail_active,
            icon_ink_disabled: t.rail_text_disabled,
            icon_fill_disabled: t.rail_fill,
            ..*t
        };
        let (icon_y, label_y) = if r.height() < ENTRY_H - 8.0 { (r.top() + 19.0, r.top() + 41.0) } else { (r.top() + 24.0, r.top() + 50.0) };
        icons::paint(painter, Rect::from_center_size(pos2(r.center().x, icon_y), vec2(26.0, 26.0)), entry.icon, &ink, enabled);
        let colour = if enabled { t.rail_text } else { t.rail_text_disabled };
        painter.text(pos2(r.center().x, label_y), Align2::CENTER_CENTER, entry.label, theme::font(size::LABEL + 0.5), colour);
        self.note(format!("backstage.{}", entry.label), r, enabled);
        // An entry says its name, and that it is not available yet when it cannot be used.
        let clicked = resp.on_hover_text(tip(entry.label, enabled)).clicked();
        if clicked && let Some(page) = entry.page {
            self.ui.new_page.detail = None;
            self.ui.backstage = Some(page);
        } else if clicked {
            match entry.act {
                Act::Open => self.pick_and_open(ui.ctx()),
                Act::Save => self.save(ui.ctx()),
                Act::SaveAs => self.save_as(ui.ctx()),
                Act::Export => self.export_dialog(ui.ctx()),
                Act::Options => self.open_options_dialog(crate::options_dialog::OptionsTab::CustomizeRibbon),
                Act::About => {
                    if let Err(e) = self.execute("app.about", &serde_json::json!({})) {
                        self.set_notice(&e.to_string());
                    }
                }
                Act::Nothing => {}
            }
        }
    }

    fn home_page(&mut self, ui: &mut Ui, t: &Tokens, body: Rect) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().id_salt("home_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.label(egui::RichText::new("Home").font(theme::font(26.0)).color(t.text));
                ui.add_space(12.0);
                // Keep the full cover visible at its original aspect ratio, reserving room for the actions
                // on short windows. Remaining content scrolls instead of overlapping or disappearing.
                let max_height = (body.height() * 0.30).clamp(0.0, 240.0);
                let width = ui.available_width().max(0.0).min(max_height * 1672.0 / 941.0);
                let (cover, _) = ui.allocate_exact_size(vec2(width, width * 941.0 / 1672.0), Sense::hover());
                self.paint_cover(ui, cover, t);
                self.note("backstage.cover", cover, false);
                ui.add_space(16.0);
                ui.label(egui::RichText::new("Start a drawing").font(theme::font(size::TAB + 1.0)).color(t.text_dim));
                ui.add_space(8.0);
                let (tile, _) = ui.allocate_exact_size(vec2(ui.available_width().clamp(0.0, 150.0), 128.0), Sense::hover());
                self.blank_tile(ui, t, tile);
                self.own_window_block(ui, t);
                ui.add_space(20.0);
                ui.label(egui::RichText::new("Recent").font(theme::font(size::TAB + 1.0)).color(t.text_dim));
                ui.add_space(8.0);
                ui.label(egui::RichText::new("No recent drawings yet.").font(theme::font(size::LABEL + 1.0)).color(t.text_disabled));
            });
        });
    }

    /// The tile that makes an empty drawing: it runs `file.new` and leaves the File view.
    fn blank_tile(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let enabled = self.command_enabled("file.new").is_ok();
        let resp = ui.interact(r, ui.scope_id().with("blank_tile"), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, BLANK_TILE);
        if resp.gained_focus() {
            resp.scroll_to_me(None);
        }
        let painter = ui.painter();
        let border = if (resp.hovered() || resp.has_focus()) && enabled { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.tile_border) };
        painter.rect(r, CornerRadius::same(6), t.backstage_tile, border, StrokeKind::Inside);
        // A page, as a drawing starts: a white sheet on the tile.
        let page = Rect::from_center_size(pos2(r.center().x, r.top() + 52.0), vec2(72.0, 54.0));
        painter.rect(page, CornerRadius::ZERO, t.page_fill, Stroke::new(1.0, t.page_border), StrokeKind::Inside);
        painter.text(pos2(r.center().x, r.bottom() - 22.0), Align2::CENTER_CENTER, BLANK_TILE, theme::font(size::LABEL + 1.0), t.text);
        self.note("backstage.tile.blank", r, enabled);
        // In a browser the tooltip names the key the tile's command has (Ctrl+N), or says the browser keeps it in a tab.
        let tooltip = self.tip_with_key(ui.ctx(), BLANK_TILE, enabled, "file.new");
        let clicked = resp.on_hover_text(tooltip).clicked() && enabled;
        if clicked {
            // Asks first if the drawing has changes that are not saved.
            self.ask_then(ui.ctx(), crate::save::Then::New);
        }
    }

    /// In a browser, where the page is in a tab: how to open DiaCraft in a window of its own, which is where the browser passes Ctrl+N, Ctrl+W and the other keys it keeps for itself to the page
    /// (`page_host.rs`). **Where it stands** (ours; no specification has a browser's window): the Home page, beside the tile that makes a drawing, because the keys are for starting and closing
    /// drawings and a person who looks for them looks here, and the reference application's File view has no place for a setting of the window (its Account and Options pages are the program's, not the
    /// window's). The block is not there on the desktop, nor in a window of its own.
    fn own_window_block(&mut self, ui: &mut Ui, t: &Tokens) {
        let offer = self.own_window_offer();
        if offer == crate::page_host::Offer::None {
            return;
        }
        ui.add_space(20.0);
        ui.label(egui::RichText::new("In a window of its own").font(theme::font(size::TAB + 1.0)).color(t.text_dim));
        ui.add_space(8.0);
        let os = ui.ctx().os();
        let key = |id: &str| crate::shortcuts::shortcut_of(os, id, true).map_or_else(String::new, |s| ui.ctx().format_shortcut(&s));
        let words = format!(
            "A browser tab keeps keys such as {} and {} for itself. In a window of its own, DiaCraft takes them: {} starts a new drawing, and {} closes the window after it asks about changes that are not saved. \
             Full Screen, on the View tab, gives it the keys as well: hold Esc to leave it.",
            key("file.new"),
            key("app.closeWindow"),
            key("file.new"),
            key("app.closeWindow")
        );
        let width = ui.available_width().clamp(0.0, 520.0);
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(egui::Label::new(egui::RichText::new(words).font(theme::font(size::LABEL + 1.0)).color(t.text)).wrap());
            ui.add_space(8.0);
            match offer {
                crate::page_host::Offer::Prompt => {
                    let label = "Open in its own window";
                    let button = egui::Button::new(egui::RichText::new(label).font(theme::font(size::LABEL + 1.0)))
                        .fill(t.accent_soft)
                        .stroke(Stroke::new(1.5, t.accent));
                    let resp = ui.add_sized(vec2(190.0, 34.0), button);
                    widgets::name_button(&resp, true, label);
                    self.note("backstage.ownWindow", resp.rect, true);
                    if resp.on_hover_text(label).clicked()
                        && let Err(e) = self.execute("app.ownWindow", &serde_json::json!({}))
                    {
                        self.set_notice(&e.to_string());
                    }
                }
                crate::page_host::Offer::Steps(steps) => {
                    let label = egui::Label::new(egui::RichText::new(steps).font(theme::font(size::LABEL + 1.0)).color(t.text_dim)).wrap();
                    let resp = ui.add(label);
                    self.note("backstage.ownWindowSteps", resp.rect, false);
                }
                crate::page_host::Offer::None => {}
            }
        });
    }
}
