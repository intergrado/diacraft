//! The row of page tabs along the bottom of the drawing area: one tab per page, then the All menu, then the button that adds a
//! page.
//!
//! A tab shows the page's name as the document holds it and switches the drawing area to the page (`page.select`). The add
//! button runs `page.add`. The All menu lists every page, which is how a page is reached when there are more tabs than the row
//! holds. **The tabs of background pages come after those of the pages that are not backgrounds** (`tab_order`), set apart by a gap and a line, and are not dragged.
//! The menu a right click opens has the six rows, each wired to the command that does it; a row that cannot run now is quiet and says why in its
//! tooltip. The Insert tab's Pages group (`run_pages_control`) works on the page in view.

use diacraft_engine::shown;
use egui::text::CCursorRange;
use egui::{Align2, CornerRadius, Frame, Id, Key, Rect, Sense, Stroke, TextEdit, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::icons::{self, Base, Icon};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg, text_width, tip};

/// The height of the row, in points.
pub const ROW_H: f32 = 28.0;

/// The most characters of a page's name a tab shows.
const NAME_CHARS: usize = 40;

/// A page tab being dragged to another place (`page.move`), and the name being typed in a tab (`page.rename`).
#[derive(Default)]
pub struct TabsState {
    drag: Option<TabDrag>,
    rename: Option<Rename>,
}

/// A foreground tab on its way to another place among the foreground tabs: the place it had and the place it would take if let go now.
struct TabDrag {
    id: u32,
    from: usize,
    to: usize,
}

/// A name being typed in a tab. **The first frame selects all of it**, so that what is typed replaces the name.
struct Rename {
    id: u32,
    original: String,
    text: String,
    fresh: bool,
}

/// The id of the box a name is typed in.
fn rename_field() -> Id {
    Id::unique("page_tab_rename")
}

/// The most characters a page's name takes in the box of a tab: a page name has at most 31 characters (the engine refuses a longer one).
const NAME_BOX_CHARS: usize = diacraft_engine::doc::MAX_PAGE_NAME_CHARS;

/// The width of the menu a right click on a tab opens, in points.
const MENU_WIDTH: f32 = 190.0;

/// The width of the All menu button and of the add button, in points.
const ALL_W: f32 = 48.0;
const ADD_W: f32 = 26.0;

/// The widest a tab is, in points.
const TAB_MAX_W: f32 = 180.0;

/// The room between two tabs, and the extra room between the last tab of a page that is not a background and the first of a background page.
const TAB_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 8.0;

/// The width of the strip of tabs of these widths: each tab and the gap after it, and the extra gap that sets the backgrounds apart when there are both kinds.
fn strip_width(widths: &[f32], foreground: usize) -> f32 {
    let set_apart = foreground > 0 && foreground < widths.len();
    widths.iter().sum::<f32>() + TAB_GAP * widths.len() as f32 + if set_apart { GROUP_GAP } else { 0.0 }
}

/// The pages in the order of the tabs: the foreground pages in the order of the file, then the background pages in theirs. Foreground
/// tabs stay left of background tabs. A project rule: no spec says how
/// a file that lists them mixed is shown.
pub(crate) fn tab_order(doc: &diacraft_engine::doc::Document) -> Vec<(u32, &str)> {
    let foreground = doc.pages.iter().filter(|p| !p.background);
    let background = doc.pages.iter().filter(|p| p.background);
    foreground.chain(background).map(|p| (p.id, &*p.name)).collect()
}

impl DiacraftApp {
    pub(crate) fn page_tabs(&mut self, ui: &mut Ui, t: Tokens) {
        let frame = Frame::new().fill(t.top_bar_fill);
        let panel = egui::Panel::bottom("page_tabs").exact_size(ROW_H).show_separator_line(false).frame(frame).show(ui, |ui| {
            let row = ui.max_rect();
            let Some(current) = self.current_page() else { return };
            let pages: Vec<(u32, String)> = self
                .session()
                .document()
                .map(|d| tab_order(d).into_iter().map(|(id, name)| (id, shown(name, NAME_CHARS))).collect())
                .unwrap_or_default();
            // A page that was taken out (Undo, the control channel) while its tab was being dragged or its name typed leaves nothing to finish.
            if self.ui.tabs.rename.as_ref().is_some_and(|r| !pages.iter().any(|(id, _)| *id == r.id)) {
                self.ui.tabs.rename = None;
            }
            if self.ui.tabs.drag.as_ref().is_some_and(|d| !pages.iter().any(|(id, _)| *id == d.id)) {
                self.ui.tabs.drag = None;
            }
            // The tabs of the pages that are not backgrounds come first (`tab_order`), and only they are moved by a drag.
            let foreground = self.session().document().map_or(0, |d| d.pages.iter().filter(|p| !p.background).count());
            // The tabs, then All, then the add button, each right after the one before while the tabs fit; when they do not,
            // the tabs scroll and All and the add button stay at the right end.
            let font = theme::font(size::BAR);
            let widths: Vec<f32> = pages.iter().map(|(_, n)| (text_width(ui, n, &font) + 26.0).min(TAB_MAX_W)).collect();
            let total = strip_width(&widths, foreground);
            let room = (row.width() - 16.0 - ALL_W - ADD_W - 10.0).max(0.0);
            let tabs = Rect::from_min_size(pos2(row.left() + 8.0, row.top()), vec2(total.min(room), row.height()));
            let all = Rect::from_min_size(pos2(tabs.right() + 6.0, row.top() + 2.0), vec2(ALL_W, ROW_H - 4.0));
            let add = Rect::from_min_size(pos2(all.right() + 4.0, row.top() + 2.0), vec2(ADD_W, ROW_H - 4.0));
            self.all_pages_menu(ui, &t, all, &pages, current);
            self.add_page_button(ui, &t, add);
            let mut child = ui.new_child(UiBuilder::new().max_rect(tabs));
            child.set_clip_rect(tabs);
            self.tab_strip(&mut child, &t, tabs, (&pages, &widths, foreground), current);
        });
        let rect = panel.response.rect;
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, t.separator));
    }

    fn tab_strip(&mut self, ui: &mut Ui, t: &Tokens, area: Rect, tabs: (&[(u32, String)], &[f32], usize), current: u32) {
        let (pages, widths, foreground) = tabs;
        let font = theme::font(size::BAR);
        let mut moved: Option<(u32, usize)> = None;
        egui::ScrollArea::horizontal()
            .id_salt("page_tab_scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let total = strip_width(widths, foreground);
                let (strip, _) = ui.allocate_exact_size(vec2(total.max(area.width()), area.height()), Sense::hover());
                let mut x = strip.left();
                let layout: Vec<Rect> = widths
                    .iter()
                    .enumerate()
                    .map(|(index, w)| {
                        let r = Rect::from_min_size(pos2(x, strip.top() + 3.0), vec2(*w, strip.height() - 3.0));
                        x += w + TAB_GAP;
                        // The tabs of background pages are set apart from the others by a gap and a line.
                        if index + 1 == foreground && foreground < widths.len() {
                            x += GROUP_GAP;
                        }
                        r
                    })
                    .collect();
                if foreground > 0
                    && let (Some(last), Some(first)) = (layout.get(foreground - 1), layout.get(foreground))
                {
                    let at = (last.right() + first.left()) / 2.0;
                    ui.painter().vline(at, (last.top() + 4.0)..=(last.bottom() - 3.0), Stroke::new(1.0, t.separator));
                }
                for (index, ((id, name), r)) in pages.iter().zip(layout.iter().copied()).enumerate() {
                    let active = *id == current;
                    if self.ui.tabs.rename.as_ref().is_some_and(|rename| rename.id == *id) {
                        self.rename_box(ui, t, r);
                        continue;
                    }
                    // A tab of a page that is not a background can be dragged to another place among them (`page.move`); a background's cannot.
                    let movable = index < foreground;
                    let sense = if movable { Sense::click_and_drag() } else { Sense::click() };
                    let resp = ui.interact(r, ui.scope_id().with(("page_tab", *id)), sense);
                    widgets::name_button(&resp, true, name);
                    self.note(format!("page-tab.{id}"), r, true);
                    let lifted = self.ui.tabs.drag.as_ref().is_some_and(|d| d.id == *id);
                    paint_tab(ui, t, r, name, active, lifted, &resp, &font);
                    // Brought into view once, when the page changes: a scroll asked for every frame can ask for what a narrow
                    // row can never give and keep the window repainting.
                    if active && self.ui.tab_scrolled_to != Some(*id) {
                        self.ui.tab_scrolled_to = Some(*id);
                        ui.scroll_to_rect(r, None);
                    }
                    if movable && resp.dragged() {
                        let pointer = ui.input(|i| i.pointer.latest_pos()).map_or(r.center().x, |p| p.x);
                        let to = drop_place(&layout, foreground, index, pointer);
                        self.ui.tabs.drag = Some(TabDrag { id: *id, from: index, to });
                    }
                    if movable
                        && resp.drag_stopped()
                        && let Some(drag) = self.ui.tabs.drag.take().filter(|d| d.id == *id && d.to != d.from)
                    {
                        moved = Some((drag.id, drag.to));
                    }
                    if resp.secondary_clicked() && !active {
                        self.select_page_by_tab(*id);
                    }
                    let tip = resp.clone().on_hover_text(if movable { name.to_owned() } else { format!("{name} (background page)") });
                    if tip.clicked() {
                        self.select_page_by_tab(*id);
                    }
                    self.page_tab_menu(&tip, *id);
                }
                if let Some(drag) = &self.ui.tabs.drag {
                    paint_drop_marker(ui, t, &layout, foreground, drag);
                }
            });
        if let Some((id, to)) = moved {
            self.move_page_tab(id, to);
        }
    }

    fn select_page_by_tab(&mut self, id: u32) {
        if let Err(e) = self.execute("page.select", &json!({ "id": id })) {
            self.set_notice(&e.to_string());
        }
    }

    /// A tab was dragged to the place `to` among the pages that are not backgrounds (`page.move`).
    fn move_page_tab(&mut self, id: u32, to: usize) {
        if let Err(e) = self.execute("page.move", &json!({ "id": id, "to": to })) {
            self.set_notice(&e.to_string());
        }
    }

    /// The menu a right click on a tab opens: Insert, Delete, Rename, Duplicate, Page Setup, a divider, Reorder Pages. Every
    /// row is wired to what does it; a row that cannot run now is quiet, and its tooltip says why.
    fn page_tab_menu(&mut self, tab: &egui::Response, id: u32) {
        let t = Tokens::get(&tab.ctx);
        let mut chosen: Option<&'static str> = None;
        egui::Popup::context_menu(tab).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            // As wide as its rows and no wider: a popup's room is the screen's, and a row that took it all would make the menu as wide.
            ui.set_width(MENU_WIDTH);
            let rows = [
                ("Insert", self.command_enabled("page.add")),
                ("Delete", self.delete_page_verdict(id)),
                ("Rename", self.command_enabled("page.rename")),
                ("Duplicate", self.command_enabled("page.duplicate")),
                ("Page Setup", self.command_enabled("window.pageSetup")),
            ];
            for (label, verdict) in &rows {
                if self.tab_menu_row(ui, &t, label, verdict) {
                    chosen = Some(*label);
                }
            }
            ui.separator();
            let reorder = self.command_enabled("window.reorderPages");
            if self.tab_menu_row(ui, &t, "Reorder Pages", &reorder) {
                chosen = Some("Reorder Pages");
            }
        });
        let Some(action) = chosen else { return };
        let result = match action {
            "Insert" => self.insert_page_like(id),
            "Delete" => self.execute("page.delete", &json!({ "id": id })).map(|answer| {
                self.ui.views.remove(&id);
                self.say_unassigned(&answer);
            }),
            "Rename" => {
                let name = self.session().document().and_then(|d| d.page(id)).map(|p| p.name.to_string()).unwrap_or_default();
                self.ui.tabs.rename = Some(Rename { id, original: name.clone(), text: name, fresh: true });
                Ok(())
            }
            "Duplicate" => self.execute("page.duplicate", &json!({ "id": id })).map(|_| ()),
            "Page Setup" => self.open_page_setup(crate::page_setup::DialogTab::PageSize, Some(id)),
            _ => {
                self.open_reorder_pages();
                Ok(())
            }
        };
        if let Err(e) = result {
            self.set_notice(&e.to_string());
        }
    }

    /// Whether the tab menu's Delete can run on page `id`: the engine's command, and a drawing keeps one page that is not a background (`page.delete` says so at its run, and the
    /// row says so before it).
    fn delete_page_verdict(&self, id: u32) -> std::result::Result<(), String> {
        self.command_enabled("page.delete")?;
        let Some(doc) = self.session().document() else { return Err("no document open".to_owned()) };
        let last = doc.page(id).is_some_and(|p| !p.background) && doc.pages.iter().filter(|p| !p.background).count() < 2;
        if last { Err("a drawing keeps one page that is not a background".to_owned()) } else { Ok(()) }
    }

    /// The status bar says when pages let go of a background page that was deleted (`page.delete`, `page.type`: the answer's `unassigned`).
    pub(crate) fn say_unassigned(&mut self, answer: &serde_json::Value) {
        let Some(n) = answer.get("unassigned").and_then(serde_json::Value::as_u64).filter(|n| *n > 0) else { return };
        let words = if n == 1 {
            "1 page had it as its background and has none now.".to_owned()
        } else {
            format!("{n} pages had it as their background and have none now.")
        };
        self.set_notice(&format!("{words} Undo puts it back."));
    }

    /// The Insert tab's Pages group: **New Page > Blank Page** adds a page like the page on view, as the plus does; **Background Page** adds a background page
    /// like it and shows it, so that what is drawn on it is where the person is; **Duplicate This Page** copies the page on view and shows the copy (`app.rs` shows a copy).
    pub(crate) fn run_pages_control(&mut self, ctx: &egui::Context, control: &str) {
        let Some(page) = self.current_page() else { return };
        let result = match control {
            "BackgroundPageInsert" => self.execute("page.add", &json!({ "background": true, "like": page })).and_then(|made| {
                let new = made.get("id").cloned().unwrap_or_default();
                self.execute("page.select", &json!({ "id": new })).map(|_| ())
            }),
            "DuplicateThisPage" => self.execute("page.duplicate", &json!({ "id": page })).map(|_| ()),
            _ => self.execute("page.add", &json!({ "like": page })).map(|_| ()),
        };
        if let Err(e) = result {
            self.set_notice(&e.to_string());
        }
        ctx.request_repaint();
    }

    /// One row of the tab menu. A row that cannot run is quiet and its tooltip says why. → whether it was pressed.
    fn tab_menu_row(&mut self, ui: &mut Ui, t: &Tokens, label: &str, verdict: &std::result::Result<(), String>) -> bool {
        let enabled = verdict.is_ok();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, label);
        paint_button_bg(ui, rect, &resp, enabled, false, t, 3);
        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), label_colour(t, enabled));
        self.note(format!("page-menu.{label}"), rect, enabled);
        if let Err(why) = verdict {
            resp.clone().on_hover_text(format!("{label} ({why})"));
        }
        resp.clicked() && enabled
    }

    /// Insert: a page that has the setup of page `id`, which is shown, and the Page Setup dialog on its Page Properties. **Ours**: the reference app opens the
    /// dialog first and makes the page when it is closed, which needs a page that does not exist yet; here the page is made first (Undo takes it away).
    fn insert_page_like(&mut self, id: u32) -> diacraft_engine::Result<()> {
        let made = self.execute("page.add", &json!({ "like": id }))?;
        let new = made.get("id").and_then(serde_json::Value::as_u64).and_then(|n| u32::try_from(n).ok());
        if let Some(new) = new {
            self.execute("page.select", &json!({ "id": new }))?;
            self.open_page_setup(crate::page_setup::DialogTab::PageProperties, Some(new))?;
        }
        Ok(())
    }

    /// The box a page's name is typed in, over its tab. Enter or a click away keeps the name (`page.rename`), Escape leaves it as it was.
    fn rename_box(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let field = rename_field();
        let ctx = ui.ctx().clone();
        let Some(rename) = self.ui.tabs.rename.as_mut() else { return };
        if ctx.memory(|m| m.focused()) != Some(field) {
            ctx.memory_mut(|m| m.request_focus(field));
        }
        if rename.fresh {
            let mut state = egui::text_edit::TextEditState::load(&ctx, field).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::select_all_str(&rename.text)));
            state.store(&ctx, field);
            rename.fresh = false;
        }
        let area = r.shrink2(vec2(2.0, 3.0));
        let out = ui.put(
            area,
            TextEdit::singleline(&mut rename.text)
                .id(field)
                .font(theme::font(size::BAR))
                .text_color(t.text)
                .char_limit(NAME_BOX_CHARS)
                .desired_width(area.width()),
        );
        out.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Page name"));
        let (escape, enter) = ui.input(|i| (i.key_pressed(Key::Escape), i.key_pressed(Key::Enter)));
        let finished = out.lost_focus() || enter || escape;
        let (id, original, typed) = (rename.id, rename.original.clone(), rename.text.trim().to_owned());
        self.note("page-tab.rename", area, true);
        if !finished {
            return;
        }
        self.ui.tabs.rename = None;
        if escape || typed.is_empty() || typed == original {
            return;
        }
        if let Err(e) = self.execute("page.rename", &json!({ "id": id, "name": typed })) {
            self.set_notice(&e.to_string());
        }
    }

    fn add_page_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let enabled = self.command_enabled("page.add").is_ok();
        let resp = ui.interact(r, ui.scope_id().with("add_page"), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, "Add Page");
        paint_button_bg(ui, r, &resp, enabled, false, t, 4);
        icons::paint(ui.painter(), r.shrink(5.0), Icon::of(Base::Page).with(icons::Badge::Plus), t, enabled);
        self.note("page-tabs.add", r, enabled);
        // The new page copies the setup of the page on view.
        let params = self.current_page().map_or_else(|| json!({}), |page| json!({ "like": page }));
        if resp.on_hover_text(tip("Add Page", enabled)).clicked()
            && let Err(e) = self.execute("page.add", &params)
        {
            self.set_notice(&e.to_string());
        }
    }

    fn all_pages_menu(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, pages: &[(u32, String)], current: u32) {
        let resp = ui.interact(r, ui.scope_id().with("all_pages"), Sense::click());
        widgets::name_button(&resp, true, "All");
        paint_button_bg(ui, r, &resp, true, false, t, 4);
        let colour = label_colour(t, true);
        ui.painter().text(pos2(r.left() + 9.0, r.center().y), Align2::LEFT_CENTER, "All", theme::font(size::BAR), colour);
        icons::chevron(ui.painter(), pos2(r.right() - 10.0, r.center().y), 2.5, colour);
        self.note("page-tabs.all", r, true);
        let mut chosen = None;
        egui::Popup::menu(&resp.on_hover_text("All pages")).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            // A drawing can have more pages than the window has room for: the list scrolls, and the arrow keys follow. Its width is its longest name's, and no less than 160 points.
            let font = egui::TextStyle::Button.resolve(ui.style());
            let widest = pages.iter().map(|(_, name)| text_width(ui, name, &font)).fold(0.0_f32, f32::max);
            let width = (widest + ui.spacing().button_padding.x * 2.0 + 8.0).max(160.0);
            widgets::menu_scroll::tall_menu(ui, t, width, |ui| {
                for (id, name) in pages {
                    let row = ui.selectable_label(*id == current, name);
                    self.note(format!("all-pages.{id}"), row.rect, true);
                    if row.clicked() {
                        chosen = Some(*id);
                    }
                }
            });
        });
        if let Some(id) = chosen
            && let Err(e) = self.execute("page.select", &json!({ "id": id }))
        {
            self.set_notice(&e.to_string());
        }
    }
}

/// The place a tab dragged from `dragged` takes if let go with the pointer at `pointer_x`: among the tabs of the pages that are not backgrounds, the
/// number of the others whose middle is left of the pointer.
fn drop_place(layout: &[Rect], foreground: usize, dragged: usize, pointer_x: f32) -> usize {
    layout.iter().take(foreground).enumerate().filter(|(i, r)| *i != dragged && r.center().x < pointer_x).count()
}

/// The line between two tabs where the tab being dragged would land.
fn paint_drop_marker(ui: &Ui, t: &Tokens, layout: &[Rect], foreground: usize, drag: &TabDrag) {
    let others: Vec<Rect> = layout.iter().take(foreground).enumerate().filter(|(i, _)| *i != drag.from).map(|(_, r)| *r).collect();
    let x = match others.get(drag.to) {
        Some(next) => next.left() - 1.0,
        None => others.last().map_or(0.0, |last| last.right() + 1.0),
    };
    if let Some(any) = layout.first() {
        ui.painter().rect_filled(Rect::from_min_max(pos2(x - 1.0, any.top()), pos2(x + 1.0, any.bottom())), CornerRadius::ZERO, t.accent);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_tab(ui: &Ui, t: &Tokens, r: Rect, name: &str, active: bool, lifted: bool, resp: &egui::Response, font: &egui::FontId) {
    let painter = ui.painter();
    let round = CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 };
    if lifted {
        painter.rect_filled(r, round, t.accent_soft);
        painter.rect_stroke(r, round, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if active {
        painter.rect_filled(r, round, t.ribbon_fill);
        painter.rect_stroke(r, round, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
        painter.rect_filled(Rect::from_min_size(r.left_top() + vec2(1.0, -1.0), vec2(r.width() - 2.0, 2.0)), 0.0, t.ribbon_fill);
    } else if resp.hovered() {
        painter.rect_filled(r, round, t.top_bar_hover);
    }
    let clip = painter.with_clip_rect(r.shrink2(vec2(4.0, 0.0)));
    clip.text(pos2(r.left() + 13.0, r.center().y), Align2::LEFT_CENTER, name, font.clone(), if active { t.text } else { t.text_dim });
}
