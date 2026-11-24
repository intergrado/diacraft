//! Drawing a group of the ribbon: its layout at each size, and each kind of control.
//!
//! A group is laid out as columns (`Col`): a large button, a stack of small ones, rows of icon buttons, a gallery. The same
//! layout gives the width the fit planner needs and the places the controls are drawn at, so the two cannot disagree. A control
//! is enabled only if a command it names can run now, or it holds a menu with such an item; every other control is drawn quiet
//! and senses no click.

use egui::{Align2, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};

use super::fit::{Level, Widths};
use super::spec::{Body, Ctl, Group, Kind, Press, Sample, Slot};
use super::{design, pickers};
use crate::app::DiacraftApp;
use crate::format::Pressed;
use crate::icons;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg, text_width, tip, two_lines};

/// The height of the ribbon body, in points: the controls and the row of group labels under them.
pub const BODY_H: f32 = 96.0;

/// The height of the row that holds a group's label.
const LABEL_ROW: f32 = 17.0;
const LABEL_PAD: f32 = 5.0;
// The 12-point launcher, its 3-point outer inset and 4 points clear of the caption.
const LABEL_LAUNCHER_SPACE: f32 = 19.0;

/// The height of the controls' area.
const CTRL_H: f32 = BODY_H - LABEL_ROW - 6.0;

const BIG_ICON: f32 = 30.0;
const SMALL_ICON: f32 = 16.0;
const ROW_H: f32 = 23.0;
const ICON_BTN: f32 = 24.0;
const GROUP_PAD: f32 = 7.0;
const FOLD_PAD: f32 = 3.0;
const COL_GAP: f32 = 3.0;
const CHEVRON_W: f32 = 12.0;

/// One column of a group.
#[derive(Clone, Debug)]
enum Col {
    Big(Ctl),
    /// Small buttons one above the other; with their labels or without.
    Stack(Vec<Ctl>, bool),
    Rows(&'static [&'static [Ctl]]),
    /// A strip of tiles; `true` for the short one.
    Gallery(Ctl, bool),
    /// The button a folded group is.
    Fold,
}

/// A group's columns, each with its width, and the group's width.
pub struct GroupLayout {
    cols: Vec<(Col, f32)>,
    /// The width of the group with its padding, and not its divider.
    pub width: f32,
}

fn label_font() -> FontId {
    theme::font(size::LABEL)
}

fn group_font() -> FontId {
    theme::font(size::GROUP)
}

/// The icon a button shows: its own, except that the Drawing Tools button shows the tool used last, as the reference's does ("the button face
/// shows the last tool used").
fn face(app: &DiacraftApp, ctl: &Ctl) -> icons::Icon {
    use diacraft_engine::draw::pen::Tool;
    if !matches!(ctl.id, "DrawingToolsMenu" | "DeveloperDrawingTools") {
        return ctl.icon;
    }
    icons::Icon::of(match app.last_drawing_tool() {
        Some(Tool::Ellipse) => icons::Base::Ellipse,
        Some(Tool::Line) => icons::Base::Line,
        Some(Tool::Freeform) => icons::Base::Freeform,
        Some(Tool::Arc) => icons::Base::Arc,
        Some(Tool::Pencil) => icons::Base::Pencil,
        Some(Tool::Rectangle) | None => icons::Base::Rect,
    })
}

/// Whether a control can be used now.
pub fn ctl_enabled(app: &DiacraftApp, ctl: &Ctl) -> bool {
    ctl.command.is_some_and(|c| app.command_enabled(c).is_ok()) || ctl.items.iter().any(|i| ctl_enabled(app, i))
}

/// Why a command a control holds cannot run now, if one of them can say.
fn reason_of(app: &DiacraftApp, ctl: &Ctl) -> Option<String> {
    ctl.command
        .filter(|c| crate::commands::known(c))
        .and_then(|c| app.command_enabled(c).err())
        .or_else(|| ctl.items.iter().find_map(|item| reason_of(app, item)))
}

/// The tooltip of a control: its caption, and for one that cannot be used the reason when its command gives one ("nothing is selected") and
/// "not available yet" when it has no command.
fn ctl_tip(app: &DiacraftApp, ctl: &Ctl, enabled: bool) -> String {
    if enabled {
        // Full Screen says how to leave it: the browser keeps Esc held for that, once the keyboard is locked (`page_host.rs`).
        if ctl.command == Some("window.fullScreen") {
            return format!("{} (hold Esc to leave)", ctl.label);
        }
        return ctl.label.to_owned();
    }
    match reason_of(app, ctl) {
        Some(reason) => format!("{} ({reason})", ctl.label),
        None => tip(ctl.label, false),
    }
}

/// The tooltip of a row of a menu that cannot be used now: the reason its command gives ("nothing is selected", the engine's words for a
/// shape operation), and for a row with no command to give one, the ribbon's one wording for a control that is not built yet, `widgets::tip`: "Join (not
/// available yet)", as a button of the ribbon says it. A row that works has none.
pub(crate) fn menu_row_tip(app: &DiacraftApp, item: &Ctl) -> String {
    reason_of(app, item).unwrap_or_else(|| tip(item.label, false))
}

fn big_width(ui: &Ui, ctl: &Ctl) -> f32 {
    let (first, second) = two_lines(ctl.label);
    let font = label_font();
    let last = second.as_deref().unwrap_or(&first);
    let chevron = if matches!(ctl.kind, Kind::Menu | Kind::Split) { CHEVRON_W } else { 0.0 };
    let widest = text_width(ui, &first, &font).max(text_width(ui, last, &font) + chevron);
    (widest + 14.0).max(46.0)
}

fn small_width(ui: &Ui, ctl: &Ctl, labels: bool) -> f32 {
    let menu = if matches!(ctl.kind, Kind::Menu | Kind::Split) { CHEVRON_W } else { 0.0 };
    match (labels, ctl.kind) {
        // A check box is drawn with its label at every size (a box alone would not say what it is), so it is as wide as its label in a compact group
        // too: a compact column of check boxes that counted an icon's width drew the labels over the next column (the narrow View tab).
        (_, Kind::Check) => 6.0 + 14.0 + 6.0 + text_width(ui, ctl.label, &label_font()) + 8.0,
        (true, _) => 6.0 + SMALL_ICON + 6.0 + text_width(ui, ctl.label, &label_font()) + menu + 8.0,
        (false, _) => ICON_BTN + menu,
    }
}

fn row_item_width(ctl: &Ctl) -> f32 {
    match (ctl.kind, ctl.id) {
        (Kind::Combo, "Font") => 104.0,
        (Kind::Combo, _) => 44.0,
        (Kind::Menu | Kind::Split, _) => ICON_BTN + 9.0,
        _ => ICON_BTN,
    }
}

fn rows_width(rows: &[&[Ctl]]) -> f32 {
    rows.iter().map(|r| r.iter().map(row_item_width).sum::<f32>() + 2.0 * r.len().saturating_sub(1) as f32).fold(0.0, f32::max)
}

fn gallery_width(ctl: &Ctl, compact: bool) -> f32 {
    design::strip_tiles(ctl.body, compact) as f32 * 46.0 + 16.0 + if compact { 6.0 } else { 0.0 }
}

fn col_width(ui: &Ui, col: &Col, group: &Group) -> f32 {
    match col {
        Col::Big(c) => big_width(ui, c),
        Col::Stack(cs, labels) => cs.iter().map(|c| small_width(ui, c, *labels)).fold(0.0, f32::max),
        Col::Rows(rows) => rows_width(rows),
        Col::Gallery(c, compact) => gallery_width(c, *compact),
        Col::Fold => {
            let (first, second) = two_lines(group.label);
            let font = group_font();
            let widest = text_width(ui, &first, &font).max(second.as_deref().map_or(0.0, |s| text_width(ui, s, &font)));
            (widest + 8.0).max(40.0)
        }
    }
}

/// The columns of `group` at `level`.
fn columns(group: &Group, level: Level) -> Vec<Col> {
    match level {
        Level::Folded => vec![Col::Fold],
        Level::Full => group
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Big(c) => Col::Big(*c),
                Slot::Stack(cs) => Col::Stack(cs.to_vec(), true),
                Slot::Rows(rows) => Col::Rows(rows),
                Slot::Gallery(c) => Col::Gallery(*c, false),
            })
            .collect(),
        Level::Compact => {
            // Every large and small control becomes an icon button, three to a column, in order; rows and galleries stay.
            let mut cols = Vec::new();
            let mut pending: Vec<Ctl> = Vec::new();
            let flush = |pending: &mut Vec<Ctl>, cols: &mut Vec<Col>| {
                for chunk in pending.chunks(3) {
                    cols.push(Col::Stack(chunk.to_vec(), false));
                }
                pending.clear();
            };
            for slot in group.slots {
                match slot {
                    Slot::Big(c) => pending.push(*c),
                    Slot::Stack(cs) => pending.extend(cs.iter().copied()),
                    Slot::Rows(rows) => {
                        flush(&mut pending, &mut cols);
                        cols.push(Col::Rows(rows));
                    }
                    Slot::Gallery(c) => {
                        flush(&mut pending, &mut cols);
                        cols.push(Col::Gallery(*c, true));
                    }
                }
            }
            flush(&mut pending, &mut cols);
            cols
        }
    }
}

/// The layout of `group` at `level`.
pub fn layout(ui: &Ui, group: &Group, level: Level) -> GroupLayout {
    let cols: Vec<(Col, f32)> = columns(group, level).into_iter().map(|c| (col_width(ui, &c, group), c)).map(|(w, c)| (c, w)).collect();
    let inner: f32 = cols.iter().map(|(_, w)| *w).sum::<f32>() + COL_GAP * cols.len().saturating_sub(1) as f32;
    let right_pad = if group.launcher.is_some() { LABEL_LAUNCHER_SPACE } else { LABEL_PAD };
    let label = if level == Level::Folded { 0.0 } else { text_width(ui, group.label, &group_font()) + LABEL_PAD + right_pad };
    // A folded group is a button and no more: it keeps the least room round it.
    let pad = if level == Level::Folded { FOLD_PAD } else { GROUP_PAD };
    GroupLayout { width: (inner + 2.0 * pad).max(label), cols }
}

/// A group's widths at each size, for the fit planner.
pub fn widths(ui: &Ui, group: &Group) -> Widths {
    Widths {
        full: layout(ui, group, Level::Full).width + 1.0,
        compact: layout(ui, group, Level::Compact).width + 1.0,
        folded: layout(ui, group, Level::Folded).width + 1.0,
    }
}

/// What a draw call needs to know about where it is.
#[derive(Clone, Copy)]
pub struct Cx {
    pub t: Tokens,
    /// The tab's label, for the names controls are known by to tests and agents.
    pub tab: &'static str,
}

/// Draws `group` at `level` with its top left at `origin`: the controls, the label under them and the group's divider.
pub fn draw_group(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, group: &Group, level: Level, origin: Pos2) -> f32 {
    let lay = layout(ui, group, level);
    let area = Rect::from_min_size(origin, vec2(lay.width, BODY_H));
    app.note(format!("ribbon.{}.group.{}", cx.tab, group.label), area, true);
    let mut x = area.left() + if level == Level::Folded { FOLD_PAD } else { GROUP_PAD };
    for (col, w) in &lay.cols {
        let h = CTRL_H;
        let rect = Rect::from_min_size(pos2(x, area.top() + 3.0), vec2(*w, h));
        draw_col(app, ui, cx, group, col, rect);
        x += w + COL_GAP;
    }
    if level != Level::Folded {
        draw_label_row(app, ui, cx, group, area);
    }
    widgets::divider(ui, pos2(area.right(), area.top() + 6.0), pos2(area.right(), area.bottom() - 6.0), &cx.t);
    lay.width + 1.0
}

fn draw_label_row(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, group: &Group, area: Rect) {
    let right_pad = if group.launcher.is_some() { LABEL_LAUNCHER_SPACE } else { LABEL_PAD };
    let label_x = (area.left() + LABEL_PAD + area.right() - right_pad) * 0.5;
    let label_pos = pos2(label_x, area.bottom() - LABEL_ROW / 2.0 - 1.0);
    let label_rect = ui.painter().text(label_pos, Align2::CENTER_CENTER, group.label, group_font(), cx.t.text_dim);
    app.note(format!("ribbon.{}.caption.{}", cx.tab, group.id), label_rect, true);
    if let Some(launcher) = &group.launcher {
        let r = Rect::from_center_size(pos2(area.right() - 9.0, label_pos.y), vec2(12.0, 12.0));
        let enabled = ctl_enabled(app, launcher);
        let resp = ui.interact(r, ui.scope_id().with(("launcher", cx.tab, group.id)), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, &format!("{} dialog launcher", group.label));
        paint_button_bg(ui, r, &resp, enabled, false, &cx.t, 2);
        let ink = Stroke::new(1.0, label_colour(&cx.t, enabled));
        let (a, b) = (r.center() + vec2(-2.5, -2.5), r.center() + vec2(2.5, 2.5));
        ui.painter().line_segment([a, b], ink);
        ui.painter().line_segment([b, b + vec2(-3.0, 0.0)], ink);
        ui.painter().line_segment([b, b + vec2(0.0, -3.0)], ink);
        app.note(format!("ribbon.{}.{}", cx.tab, launcher.id), r, enabled);
        if resp.clone().on_hover_text(ctl_tip(app, launcher, enabled)).clicked() && enabled {
            press(app, ui, launcher);
        }
    }
}

fn draw_col(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, group: &Group, col: &Col, rect: Rect) {
    match col {
        Col::Big(c) => draw_big(app, ui, cx, c, rect),
        Col::Stack(cs, labels) => {
            let n = cs.len().clamp(1, 3);
            let h = ROW_H.min(rect.height() / n as f32);
            let top = rect.top() + (rect.height() - h * cs.len() as f32) / 2.0;
            for (i, c) in cs.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.left(), top + h * i as f32), vec2(rect.width(), h));
                draw_small(app, ui, cx, c, r, *labels);
            }
        }
        Col::Rows(rows) => {
            let total = ROW_H * rows.len() as f32;
            let top = rect.top() + (rect.height() - total) / 2.0;
            for (ri, row) in rows.iter().enumerate() {
                let mut x = rect.left();
                for c in row.iter() {
                    let w = row_item_width(c);
                    let r = Rect::from_min_size(pos2(x, top + ROW_H * ri as f32), vec2(w, ROW_H));
                    draw_row_item(app, ui, cx, c, r);
                    x += w + 2.0;
                }
            }
        }
        Col::Gallery(c, compact) => draw_gallery(app, ui, cx, c, rect, *compact),
        Col::Fold => draw_fold(app, ui, cx, group, rect),
    }
}

/// Opens the menu of `ctl` under `resp` and draws its items; a click on an enabled item runs its command. A colour control's menu opens with
/// its palette (`pickers`).
fn menu_popup(app: &mut DiacraftApp, ui: &Ui, cx: Cx, resp: &egui::Response, ctl: &Ctl) {
    if ctl.items.is_empty() && ctl.palette.is_none() && ctl.body == Body::None {
        return;
    }
    let control = *ctl;
    // A palette has a box to type a colour in, which a click must not close; an item closes the menu itself. So does the Themes gallery, whose tiles
    // take a right click (a context menu of their own).
    let behaviour = if control.palette.is_some() || control.body == Body::Themes {
        egui::PopupCloseBehavior::CloseOnClickOutside
    } else {
        egui::PopupCloseBehavior::CloseOnClick
    };
    let shown = egui::Popup::menu(resp).close_behavior(behaviour).show(|ui| {
        ui.set_width(menu_width(ui, control.items, control.palette.is_some()).max(if control.body == Body::PageSizes {
            pickers::SIZES_WIDTH
        } else {
            design::body_width(control.body)
        }));
        if let Some(palette) = control.palette {
            super::palette::colour_menu(app, ui, cx, &control, palette);
            if !control.items.is_empty() {
                ui.add_space(2.0);
                ui.separator();
            }
        }
        if control.body == Body::PageSizes {
            pickers::size_gallery(app, ui, cx);
            if !control.items.is_empty() {
                ui.add_space(2.0);
                ui.separator();
            }
        }
        if control.body == Body::ContainerStyles {
            crate::container_ui::style_menu(app, ui, cx, &control);
        }
        if control.body == Body::Masters {
            super::change_shape::gallery_menu(app, ui, cx);
        }
        if matches!(control.body, Body::Themes | Body::Variants) {
            design::gallery_menu(app, ui, cx, &control);
            if !control.items.is_empty() {
                ui.add_space(2.0);
                ui.separator();
            }
        }
        if control.body == Body::Callouts {
            super::callouts::gallery_menu(app, ui, cx);
        }
        if control.body == Body::Layouts {
            crate::relayout_ui::gallery_menu(app, ui, cx);
        }
        if control.body == Body::QuickStyles {
            super::quick_styles::gallery_menu(app, ui, cx);
            if !control.items.is_empty() {
                ui.add_space(2.0);
                ui.separator();
            }
        }
        for item in control.items {
            menu_row(app, ui, cx, item);
        }
    });
    // The theme a right click picked in the Themes menu is forgotten when the menu closes.
    if shown.is_none() && control.body == Body::Themes {
        app.ui.design.picked = None;
    }
    let _ = ui;
}

/// The room a submenu's list leaves for its frame and a little air at the window's top and bottom edges, in points.
const SUBMENU_MARGIN: f32 = 24.0;

/// The least a submenu's list is tall, in points, in a window too short for more: four rows.
const SUBMENU_MIN_HEIGHT: f32 = 112.0;

/// How tall a submenu's list may be in a window `window_h` points tall before it scrolls: as tall as the window less [`SUBMENU_MARGIN`], and
/// never less than [`SUBMENU_MIN_HEIGHT`]. **The longest list, the 23 dashes, is all on the screen in a window 800 points tall or more**: the menu moves up
/// to fit, as every menu does. A fixed 330 points hid rows 13 to 23 of it in a window of any size.
pub(crate) fn submenu_height(window_h: f32) -> f32 {
    let room = window_h - SUBMENU_MARGIN;
    if room.is_finite() { room.max(SUBMENU_MIN_HEIGHT) } else { SUBMENU_MIN_HEIGHT }
}

/// The least a menu is wide, in points.
const MENU_MIN_W: f32 = 170.0;

/// How wide the menu of `items` is: its widest row (room for an icon or a sample, the label, and a chevron), or the palette over it. **A menu is
/// as wide as this and no wider**: a row that took all the width there is would make the menu as wide as the screen.
pub(crate) fn menu_width(ui: &Ui, items: &[Ctl], palette: bool) -> f32 {
    let font = label_font();
    let rows = items
        .iter()
        .map(|item| {
            let left = if item.sample == Sample::Icon { 30.0 } else { 8.0 + pickers::SAMPLE_W + 10.0 };
            left + text_width(ui, item.label, &font) + 28.0
        })
        .fold(0.0_f32, f32::max);
    rows.max(if palette { super::palette::WIDTH } else { 0.0 }).max(MENU_MIN_W)
}

pub(crate) fn menu_row(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, item: &Ctl) {
    let enabled = ctl_enabled(app, item);
    let checked = item.kind == Kind::Check && app.check_state(item);
    let current = pickers::is_current(app, item);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
    if item.kind == Kind::Check {
        widgets::name_check(&resp, enabled, checked, item.label);
    } else if current {
        widgets::name_toggle(&resp, enabled, true, item.label);
    } else {
        widgets::name_button(&resp, enabled, item.label);
    }
    paint_button_bg(ui, rect, &resp, enabled, current, &cx.t, 3);
    let painter = ui.painter();
    let mut text_x = rect.left() + 30.0;
    if item.kind == Kind::Check {
        let b = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(13.0, 13.0));
        // The box of the ribbon's check boxes (`draw_small`): the accent filled with a tick when it is on.
        painter.rect(
            b,
            CornerRadius::same(2),
            if checked { cx.t.accent } else { cx.t.input_fill },
            Stroke::new(1.0, if enabled { cx.t.input_border } else { cx.t.text_disabled }),
            StrokeKind::Inside,
        );
        if checked {
            let (a, m, z) = (b.left_top() + vec2(2.5, 6.5), b.left_top() + vec2(5.5, 9.5), b.left_top() + vec2(10.5, 3.5));
            painter.line_segment([a, m], Stroke::new(1.6, cx.t.accent_text));
            painter.line_segment([m, z], Stroke::new(1.6, cx.t.accent_text));
        }
    } else if item.sample != Sample::Icon {
        let area = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top()), vec2(pickers::SAMPLE_W, rect.height()));
        pickers::paint_sample(painter, area, item.sample, &cx.t, enabled);
        text_x = area.right() + 10.0;
    } else {
        icons::paint(painter, Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(16.0)), item.icon, &cx.t, enabled);
    }
    painter.text(pos2(text_x, rect.center().y), Align2::LEFT_CENTER, item.label, label_font(), label_colour(&cx.t, enabled));
    let opens_menu = !item.items.is_empty() || item.body == Body::VariantColours;
    if opens_menu {
        icons::chevron(painter, pos2(rect.right() - 10.0, rect.center().y), 2.5, label_colour(&cx.t, enabled));
    }
    app.note(format!("menu.{}", item.id), rect, enabled);
    if !enabled {
        resp.clone().on_hover_text(menu_row_tip(app, item));
    }
    if opens_menu {
        // A menu inside a menu opens beside its row. It is as tall as the window lets it be, and in a window too short for it, it scrolls with a bar that
        // is always drawn: egui's own bar shows only under the pointer, so a person saw no sign of the rows below.
        if enabled {
            egui::containers::menu::SubMenu::new().show(ui, &resp, |ui| {
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                // egui paints the bar's handle in the fill of an idle widget, which the theme makes the list's own white: the handle takes the input border's
                // grey, so that it shows. (The rows paint themselves from the tokens and use no egui widget.)
                ui.visuals_mut().widgets.inactive.bg_fill = cx.t.input_border;
                // (Room for the scroll bar a long list has.)
                let wide = menu_width(ui, item.items, false).max(if item.body == Body::VariantColours { 220.0 } else { 0.0 });
                ui.set_width(wide + ui.spacing().scroll.allocated_width());
                // As tall as its rows, up to `height`. Not cut to the room under the row the menu opened from (what the popup's last size leaves it): a
                // list taller than that is moved up to fit the window, as any menu is.
                let height = submenu_height(ui.ctx().content_rect().height());
                egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([true, true]).show(ui, |ui| {
                    if item.body == Body::VariantColours {
                        design::colour_rows(app, ui, cx);
                        ui.separator();
                    }
                    for sub in item.items {
                        menu_row(app, ui, cx, sub);
                    }
                });
            });
        }
    } else if resp.clicked() && enabled {
        app.run_ribbon_command(ui.ctx(), item);
        chosen(app, ui);
    }
}

/// Runs what a press on `ctl` does: its command, if it has one. A command run from the card of a folded group closes the card, as a row chosen in
/// a menu does: a button, a tool picked, a toggle such as Bold, the launcher and the split button's icon all agree with its menu, and a tool does not leave the
/// card over the drawing. **A check box does not**: the card stays so that a person can tick several in a row (Ruler, then Grid).
fn press(app: &mut DiacraftApp, ui: &Ui, ctl: &Ctl) {
    app.run_ribbon_command(ui.ctx(), ctl);
    if ctl.kind != Kind::Check {
        close_fold_card(app, ui.ctx());
    }
}

/// A large split button (Paste): the icon runs the control's command, and the label with its arrow under the icon opens the menu. Each part is
/// quiet by itself: with an empty clipboard Paste does nothing and the menu can still hold what applies (Paste Duplicate).
fn draw_big_split(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, rect: Rect) {
    let main_on = ctl.command.is_some_and(|c| app.command_enabled(c).is_ok());
    let menu_on = ctl.items.iter().any(|item| ctl_enabled(app, item));
    let split_y = rect.top() + 4.0 + BIG_ICON + 3.0;
    let main = Rect::from_min_max(rect.min, pos2(rect.max.x, split_y));
    let under = Rect::from_min_max(pos2(rect.min.x, split_y), rect.max);
    let id = ui.scope_id().with(("big", cx.tab, ctl.id));
    let main_resp = ui.interact(main, id.with("main"), if main_on { Sense::click() } else { Sense::hover() });
    let menu_resp = ui.interact(under, id.with("menu"), if menu_on { Sense::click() } else { Sense::hover() });
    widgets::name_button(&main_resp, main_on, ctl.label);
    widgets::name_button(&menu_resp, menu_on, &format!("{} options", ctl.label));
    paint_button_bg(ui, main, &main_resp, main_on, false, &cx.t, 4);
    paint_button_bg(ui, under, &menu_resp, menu_on, false, &cx.t, 4);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.0 + BIG_ICON / 2.0), Vec2::splat(BIG_ICON)),
        ctl.icon,
        &cx.t,
        main_on,
    );
    let colour = label_colour(&cx.t, menu_on || main_on);
    let font = label_font();
    ui.painter().text(pos2(rect.center().x, split_y), Align2::CENTER_TOP, ctl.label, font.clone(), colour);
    let tw = text_width(ui, ctl.label, &font);
    icons::chevron(ui.painter(), pos2(rect.center().x + tw / 2.0 + 7.0, split_y + 7.0), 2.5, colour);
    main_resp.clone().on_hover_text(ctl_tip(app, ctl, main_on));
    menu_resp.clone().on_hover_text(if menu_on { format!("{} options", ctl.label) } else { ctl_tip(app, ctl, false) });
    let any = main_on || menu_on;
    app.note(format!("ribbon.{}.{}", cx.tab, ctl.id), rect, any);
    app.note(format!("ribbon.{}.{}.main", cx.tab, ctl.id), main, main_on);
    app.note(format!("ribbon.{}.{}.menu", cx.tab, ctl.id), under, menu_on);
    if main_resp.clicked() && main_on {
        press(app, ui, ctl);
    }
    menu_popup(app, ui, cx, &menu_resp, ctl);
}

fn draw_big(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, rect: Rect) {
    if ctl.kind == Kind::Split && ctl.command.is_some() {
        return draw_big_split(app, ui, cx, ctl, rect);
    }
    let enabled = ctl_enabled(app, ctl);
    let resp = ui.interact(rect, ui.scope_id().with(("big", cx.tab, ctl.id)), if enabled { Sense::click() } else { Sense::hover() });
    // A large button that is a toggle (Auto Size) shows whether it is on.
    let pressed = ctl.press != Press::None && app.ctl_pressed(ctl) == Pressed::On;
    if ctl.press == Press::None {
        widgets::name_button(&resp, enabled, ctl.label);
    } else {
        widgets::name_toggle(&resp, enabled, pressed, ctl.label);
    }
    paint_button_bg(ui, rect, &resp, enabled, pressed, &cx.t, 4);
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.0 + BIG_ICON / 2.0), Vec2::splat(BIG_ICON));
    icons::paint(ui.painter(), icon_rect, ctl.icon, &cx.t, enabled);
    let (first, second) = two_lines(ctl.label);
    let colour = label_colour(&cx.t, enabled);
    let y1 = rect.top() + 4.0 + BIG_ICON + 3.0;
    let font = label_font();
    ui.painter().text(pos2(rect.center().x, y1), Align2::CENTER_TOP, &first, font.clone(), colour);
    let last_y = if second.is_some() { y1 + 13.0 } else { y1 };
    if let Some(second) = &second {
        ui.painter().text(pos2(rect.center().x, last_y), Align2::CENTER_TOP, second, font.clone(), colour);
    }
    if matches!(ctl.kind, Kind::Menu | Kind::Split) {
        let last = second.as_deref().unwrap_or(&first);
        let tw = text_width(ui, last, &font);
        icons::chevron(ui.painter(), pos2(rect.center().x + tw / 2.0 + 7.0, last_y + 7.0), 2.5, colour);
    }
    resp.clone().on_hover_text(ctl_tip(app, ctl, enabled));
    app.note(format!("ribbon.{}.{}", cx.tab, ctl.id), rect, enabled);
    if matches!(ctl.kind, Kind::Menu | Kind::Split) {
        menu_popup(app, ui, cx, &resp, ctl);
    } else if resp.clicked() && enabled {
        press(app, ui, ctl);
    }
}

/// How wide the strip at the right of a small split button is: the arrow that opens its menu.
const SPLIT_STRIP: f32 = 11.0;

fn draw_small(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, rect: Rect, labels: bool) {
    // A small split button (Bullets): its icon runs the command, a narrow strip at its right opens the menu, and each is quiet by itself.
    let split = ctl.kind == Kind::Split && ctl.command.is_some();
    let main_on = ctl.command.is_some_and(|c| app.command_enabled(c).is_ok());
    let enabled = if split { main_on } else { ctl_enabled(app, ctl) };
    let strip = Rect::from_min_max(pos2(rect.right() - SPLIT_STRIP, rect.min.y), rect.max);
    let main = if split { Rect::from_min_max(rect.min, pos2(strip.left(), rect.max.y)) } else { rect };
    let checked = ctl.kind == Kind::Check && app.check_state(ctl);
    let resp = ui.interact(main, ui.scope_id().with(("small", cx.tab, ctl.id)), if enabled { Sense::click() } else { Sense::hover() });
    let menu_on = split && ctl.items.iter().any(|item| ctl_enabled(app, item));
    let menu_resp = split.then(|| {
        let r = ui.interact(strip, ui.scope_id().with(("small-menu", cx.tab, ctl.id)), if menu_on { Sense::click() } else { Sense::hover() });
        widgets::name_button(&r, menu_on, &format!("{} options", ctl.label));
        r
    });
    let pressed = app.ctl_pressed(ctl);
    if ctl.kind == Kind::Check {
        widgets::name_check(&resp, enabled, checked, ctl.label);
    } else if ctl.press != super::spec::Press::None {
        widgets::name_toggle(&resp, enabled, pressed == Pressed::On, ctl.label);
    } else {
        widgets::name_button(&resp, enabled, ctl.label);
    }
    paint_button_bg(ui, rect, &resp, enabled, pressed == Pressed::On, &cx.t, 3);
    if pressed == Pressed::Part {
        // Some of the shapes have it: the outline of a pressed button, and none of its fill.
        ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, cx.t.accent), StrokeKind::Inside);
    }
    let bar = pickers::bar_colour(app, ctl);
    let painter = ui.painter();
    let colour = label_colour(&cx.t, enabled);
    if ctl.kind == Kind::Check {
        let b = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(14.0, 14.0));
        let border = if enabled { cx.t.input_border } else { cx.t.text_disabled };
        painter.rect(b, CornerRadius::same(2), if checked { cx.t.accent } else { cx.t.input_fill }, Stroke::new(1.0, border), StrokeKind::Inside);
        if checked {
            let (a, m, z) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
            painter.line_segment([a, m], Stroke::new(1.6, cx.t.accent_text));
            painter.line_segment([m, z], Stroke::new(1.6, cx.t.accent_text));
        }
        painter.text(pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, ctl.label, label_font(), colour);
    } else if labels {
        let icon_c = pos2(rect.left() + 6.0 + SMALL_ICON / 2.0, rect.center().y);
        icons::paint(painter, Rect::from_center_size(icon_c, Vec2::splat(SMALL_ICON)), face(app, ctl), &cx.t, enabled);
        if let Some(mix) = &bar {
            pickers::paint_bar(painter, Rect::from_min_max(icon_c + vec2(-7.0, 6.0), icon_c + vec2(7.0, 9.5)), mix, &cx.t, enabled);
        }
        painter.text(pos2(rect.left() + 6.0 + SMALL_ICON + 6.0, rect.center().y), Align2::LEFT_CENTER, ctl.label, label_font(), colour);
        if matches!(ctl.kind, Kind::Menu | Kind::Split) {
            icons::chevron(painter, pos2(rect.right() - 9.0, rect.center().y), 2.5, colour);
        }
    } else {
        let icon_c = pos2(rect.left() + ICON_BTN / 2.0, rect.center().y);
        icons::paint(painter, Rect::from_center_size(icon_c, Vec2::splat(SMALL_ICON + 2.0)), face(app, ctl), &cx.t, enabled);
        if let Some(mix) = &bar {
            pickers::paint_bar(painter, Rect::from_min_max(icon_c + vec2(-7.0, 6.5), icon_c + vec2(7.0, 10.0)), mix, &cx.t, enabled);
        }
        if matches!(ctl.kind, Kind::Menu | Kind::Split) {
            icons::chevron(painter, pos2(rect.right() - 7.0, rect.center().y), 2.0, colour);
        }
    }
    resp.clone().on_hover_text(ctl_tip(app, ctl, enabled));
    app.note(format!("ribbon.{}.{}", cx.tab, ctl.id), rect, enabled || menu_on);
    if let Some(menu_resp) = menu_resp {
        app.note(format!("ribbon.{}.{}.main", cx.tab, ctl.id), main, enabled);
        app.note(format!("ribbon.{}.{}.menu", cx.tab, ctl.id), strip, menu_on);
        menu_resp.clone().on_hover_text(if menu_on { format!("{} options", ctl.label) } else { ctl_tip(app, ctl, false) });
        if resp.clicked() && enabled {
            press(app, ui, ctl);
        }
        menu_popup(app, ui, cx, &menu_resp, ctl);
    } else if matches!(ctl.kind, Kind::Menu | Kind::Split) {
        menu_popup(app, ui, cx, &resp, ctl);
    } else if resp.double_clicked() && enabled && ctl.command == Some("format.painter") {
        // A double click on the Format Painter keeps it on until Escape (`painter.rs`).
        app.run_ribbon_params(ui.ctx(), "format.painter", &json!({"on": true, "keep": true}));
        close_fold_card(app, ui.ctx());
    } else if resp.clicked() && enabled {
        press(app, ui, ctl);
    }
}

fn draw_row_item(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, rect: Rect) {
    if ctl.kind != Kind::Combo {
        return draw_small(app, ui, cx, ctl, rect, false);
    }
    // A box with a drop-down arrow, for a font or a size: it shows what the selection has (nothing when the shapes differ) and opens
    // a list (`pickers`). Its caption is its hint while nothing is selected.
    let enabled = ctl_enabled(app, ctl);
    let resp = ui.interact(rect, ui.scope_id().with(("combo", cx.tab, ctl.id)), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, ctl.label);
    let r = rect.shrink2(vec2(0.0, 1.0));
    ui.painter().rect(r, CornerRadius::same(3), cx.t.input_fill, Stroke::new(1.0, cx.t.input_border), StrokeKind::Inside);
    paint_button_bg(ui, r, &resp, enabled, false, &cx.t, 3);
    let colour = label_colour(&cx.t, enabled);
    let shown = if enabled {
        match ctl.id {
            "Font" => pickers::font_label(app),
            "FontSize" => pickers::size_label(app),
            _ => None,
        }
    } else {
        Some(ctl.label.to_owned())
    };
    if let Some(text) = shown {
        let clip = Rect::from_min_max(r.min, pos2(r.right() - 16.0, r.max.y));
        ui.painter().with_clip_rect(clip).text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, text, label_font(), colour);
    }
    icons::chevron(ui.painter(), pos2(r.right() - 8.0, r.center().y), 2.5, colour);
    app.note(format!("ribbon.{}.{}", cx.tab, ctl.id), rect, enabled);
    let control = *ctl;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| match control.id {
        "Font" => pickers::font_menu(app, ui, cx, &control),
        "FontSize" => pickers::size_menu(app, ui, cx, &control),
        _ => {}
    });
    resp.on_hover_text(ctl_tip(app, ctl, enabled));
}

fn draw_gallery(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, rect: Rect, compact: bool) {
    let enabled = ctl_enabled(app, ctl);
    let resp = ui.interact(rect, ui.scope_id().with(("gallery", cx.tab, ctl.id)), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, ctl.label);
    let strip = rect.shrink2(vec2(0.0, 4.0));
    ui.painter().rect(strip, CornerRadius::same(3), cx.t.input_fill, Stroke::new(1.0, cx.t.input_border), StrokeKind::Inside);
    let tiles = if compact { 1 } else { 3 };
    let (fill, accent) = if enabled { (cx.t.icon_fill, cx.t.icon_accent) } else { (cx.t.icon_fill_disabled, cx.t.icon_ink_disabled) };
    if matches!(ctl.body, Body::Themes | Body::Variants) {
        // The Design tab's galleries draw the tiles of the engine's themes and variants, and each tile is a control (`ribbon/design.rs`).
        design::strip(app, ui, cx, ctl, strip, compact);
    } else {
        for i in 0..tiles {
            let x = strip.left() + 4.0 + i as f32 * 46.0;
            let tile = Rect::from_min_size(pos2(x, strip.top() + 4.0), vec2(42.0, strip.height() - 8.0));
            ui.painter().rect_filled(tile, CornerRadius::same(2), fill);
            ui.painter().text(pos2(tile.center().x, tile.top() + 6.0), Align2::CENTER_TOP, "Aa", theme::font(12.0), label_colour(&cx.t, enabled));
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(tile.left() + 6.0, tile.bottom() - 12.0), vec2(30.0, 4.0)),
                CornerRadius::same(1),
                accent,
            );
        }
    }
    let arrow = Rect::from_min_max(pos2(strip.right() - 16.0, strip.top()), strip.max);
    ui.painter().line_segment([arrow.left_top(), arrow.left_bottom()], Stroke::new(1.0, cx.t.input_border));
    icons::chevron(ui.painter(), arrow.center(), 2.5, label_colour(&cx.t, enabled));
    resp.clone().on_hover_text(ctl_tip(app, ctl, enabled));
    app.note(format!("ribbon.{}.{}", cx.tab, ctl.id), rect, enabled);
    app.note(format!("ribbon.{}.{}.more", cx.tab, ctl.id), arrow, enabled);
    menu_popup(app, ui, cx, &resp, ctl);
}

/// The button a folded group is: its icon over its label and an arrow, opening the whole group under it.
fn draw_fold(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, group: &Group, rect: Rect) {
    let resp = ui.interact(rect, ui.scope_id().with(("fold", cx.tab, group.id)), Sense::click());
    widgets::name_button(&resp, true, group.label);
    paint_button_bg(ui, rect, &resp, true, false, &cx.t, 4);
    let any = {
        let mut any = false;
        group.walk(&mut |c| any |= ctl_enabled(app, c));
        any
    };
    // The icon with the arrow beside it, and the name under them in at most two lines.
    let icon_rect = Rect::from_center_size(pos2(rect.center().x - 5.0, rect.top() + 4.0 + 12.0), Vec2::splat(24.0));
    icons::paint(ui.painter(), icon_rect, group.icon, &cx.t, any);
    let colour = label_colour(&cx.t, true);
    icons::chevron(ui.painter(), pos2(icon_rect.right() + 6.0, icon_rect.center().y + 1.0), 2.5, colour);
    let (first, second) = two_lines(group.label);
    let y = rect.top() + 4.0 + 24.0 + 4.0;
    ui.painter().text(pos2(rect.center().x, y), Align2::CENTER_TOP, &first, group_font(), colour);
    if let Some(second) = &second {
        ui.painter().text(pos2(rect.center().x, y + 12.0), Align2::CENTER_TOP, second, group_font(), colour);
    }
    resp.clone().on_hover_text(group.label);
    app.note(format!("ribbon.{}.fold.{}", cx.tab, group.id), rect, true);
    fold_card(app, ui, cx, group, &resp);
}

/// Whether a click this frame landed outside the card and everything open over it: on a layer that is not a popup's. The card and the menus opened from
/// its controls are popups (`Order::Foreground`); the ribbon, the drawing and the bars are not.
fn clicked_outside_popups(ctx: &egui::Context) -> bool {
    let (clicked, at) = ctx.input(|i| (i.pointer.any_click(), i.pointer.interact_pos()));
    if !clicked {
        return false;
    }
    let Some(at) = at else { return true };
    !ctx.layer_id_at(at).is_some_and(|layer| layer.order == egui::Order::Foreground)
}

/// The card a folded group's button opens: the whole group as designed, in a popup under the button.
///
/// **Why the card is not egui's usual menu popup.** egui keeps one open popup in its memory, and a menu opened from a control inside the card (Align,
/// Operations, the arrow of Paste) takes that place: the card, which held it, was closed in the next frame and its menu with it, so no menu of a folded
/// group could be opened. The card's own state is therefore kept here (`UiState::fold_card`, opened by a press on the button) and egui's memory is left
/// to the menu over it. What egui's popup did by itself is done here:
///
/// * **a press on the button** opens the card or closes it;
/// * **a click outside** the card and the menus over it closes it ([`clicked_outside_popups`]), and a click on the card itself, away from its controls,
///   keeps it;
/// * **a row chosen** in a menu opened from the card closes the menu and the card ([`chosen`]), and **so does a command run by any other control of the
///   card** (a button, a tool picked, a toggle such as Bold, the launcher, the icon of a split button: [`press`]), so that a split button's
///   icon and its menu agree and a tool is not picked with the card left over the drawing; **a check box ticked leaves the card open**, so that several can be
///   ticked in a row;
/// * **Escape** closes the menu over the card first and the card only when no menu is open (egui's popup closes on any Escape, so the card is kept
///   when a menu was open as the key came); and every key of the drawing (Escape, the arrows, F2, typing) stands aside while a card or a menu is open
///   (`DiacraftApp::menu_open`, in `edit/keys.rs`);
/// * a dialog that opens closes the card, and so does a pass that does not draw the card's button (another tab, a window with room for the group).
fn fold_card(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, group: &Group, button: &egui::Response) {
    let ctx = ui.ctx().clone();
    let card = egui::Popup::default_response_id(button);
    let mut open = app.open_fold_card(&ctx) == Some(card);
    if button.clicked() {
        open = !open;
    } else if open && (clicked_outside_popups(&ctx) || app.is_modal_open()) {
        open = false;
    }
    // Asked before this frame's menus are drawn: egui's memory still holds the menu that was open as the key came.
    let menu_was_open = egui::Popup::is_any_open(&ctx);
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if !open {
        if app.ui.fold_card.is_some_and(|(id, _)| id == card) {
            app.ui.fold_card = None;
        }
        return;
    }
    app.ui.fold_card = Some((card, ctx.cumulative_pass_nr()));
    let mut shown = true;
    let (group, cx2) = (*group, cx);
    egui::Popup::menu(button).open_bool(&mut shown).close_behavior(egui::PopupCloseBehavior::IgnoreClicks).show(|ui| {
        let lay = layout(ui, &group, Level::Full);
        let (rect, _) = ui.allocate_exact_size(vec2(lay.width + 1.0, BODY_H), Sense::hover());
        draw_group(app, ui, cx2, &group, Level::Full, rect.min);
    });
    if !shown && escape && menu_was_open {
        // The Escape was the menu's, which closed with it: the card stays.
        shown = true;
    }
    if !shown && app.ui.fold_card.is_some_and(|(id, _)| id == card) {
        app.ui.fold_card = None;
        ctx.request_repaint();
    }
}

/// A row of a menu was chosen and its command ran: the menu closes, and so does the card of a folded group the menu was opened from.
pub(crate) fn chosen(app: &mut DiacraftApp, ui: &Ui) {
    ui.close();
    close_fold_card(app, ui.ctx());
}

/// Closes the card of a folded group, if one is open (the next frame does not draw it); nothing to do for a group as designed.
fn close_fold_card(app: &mut DiacraftApp, ctx: &egui::Context) {
    app.ui.fold_card = None;
    ctx.request_repaint();
}

impl DiacraftApp {
    /// The card of the folded group that is open, if one is: it counts as open only while the last pass or this one drew it, so a card whose button is not
    /// drawn any more (another tab, a window with room for the group, the File view) is closed (`UiState::fold_card`).
    pub(crate) fn open_fold_card(&self, ctx: &egui::Context) -> Option<egui::Id> {
        let (card, pass) = self.ui.fold_card?;
        (ctx.cumulative_pass_nr().saturating_sub(pass) <= 1).then_some(card)
    }

    /// Whether a menu is open, or the card of a folded group: the keys of the drawing stand aside, as Escape closes it first. (The card is not in egui's
    /// memory of open popups: a menu over it must be.)
    pub(crate) fn menu_open(&self, ctx: &egui::Context) -> bool {
        egui::Popup::is_any_open(ctx) || self.open_fold_card(ctx).is_some()
    }

    /// A check box's state: whether what its command toggles is on.
    pub(crate) fn check_state(&self, ctl: &Ctl) -> bool {
        match ctl.command {
            Some("view.rulers") => self.ui.show_rulers,
            Some("view.grid") => self.ui.show_grid,
            Some("view.guides") => self.ui.show_guides,
            Some("view.connectionPoints") => self.ui.show_connection_points,
            // The drawing's AutoConnect box (the program's setting over it: off, the box is shown clear).
            Some("view.autoConnect") => self.autoconnect_box(),
            // The drawing's Page Breaks box, and the page's Avoid Page Breaks (`relayout_ui.rs`).
            Some("view.pageBreaks") => self.page_breaks_box(),
            Some("arrange.avoidPageBreaks") => self.avoid_breaks_checked(),
            // A Snap & Glue option of the document (Dynamic Grid): the one its parameters name, as the document holds it now.
            Some("snap.set") => self.snap_box_checked(ctl.args),
            // Show Line Jumps: the page's jumps are on (`ribbon/design.rs`).
            Some("connector.line_jumps") => self.ui.design.jumps,
            // A Reroute choice of a connector's menu: the one the connectors selected have (`edit/legs.rs`).
            Some("connector.reroute") => {
                serde_json::from_str::<Value>(ctl.args).ok().and_then(|v| v["mode"].as_str().map(str::to_owned)).as_deref() == self.selected_reroute()
            }
            // Lock Container: every container selected is locked (`container_ui.rs`).
            Some("window.containerDo") => self.container_locked(),
            // Hide Picture: the picture of the position selected is hidden (`org_ui.rs`).
            Some("window.orgChartDo") => self.org_picture_hidden(),
            Some("window.shapes") => !self.ui.shapes_collapsed,
            Some("window.fullScreen") => self.is_full_screen(),
            Some("window.pane") => self.pane_checked(ctl.args),
            // Show Markup: the markers of the comments show on the page (`comments_ui.rs`).
            Some("comment.markers") => self.comment_markers_on(),
            _ => false,
        }
    }

    /// The window's context menu of the drawing area: the rows a right click offers (`spec::CANVAS_MENU`), and when the shapes selected are connectors the connector's own
    /// (`spec::CONNECTOR_MENU`).
    pub(crate) fn canvas_menu_rows(&mut self, ui: &mut Ui) {
        let cx = Cx { t: crate::theme::Tokens::get(ui.ctx()), tab: "canvas" };
        let rows = if self.selection_is_connectors() {
            super::spec::CONNECTOR_MENU
        } else if self.selection_is_swimlane_part() {
            super::spec::LANE_MENU
        } else {
            super::spec::CANVAS_MENU
        };
        ui.set_width(menu_width(ui, rows, false));
        for item in rows {
            menu_row(self, ui, cx, item);
        }
    }

    /// How far the control is pressed: the tool in use, or the style or alignment the selection has (`format.rs`).
    pub(crate) fn ctl_pressed(&self, ctl: &Ctl) -> Pressed {
        if self.is_current_tool(ctl.command) { Pressed::On } else { self.press_state(ctl.press) }
    }

    /// Runs the command a ribbon control names with the parameters it carries, and tells the person if it fails.
    pub(crate) fn run_ribbon_command(&mut self, ctx: &egui::Context, ctl: &Ctl) {
        let Some(command) = ctl.command else { return };
        // A command that needs a file (`file.export` takes a path) asks for one with the native dialog first, as the File view does.
        if command == "file.export" {
            self.export_dialog(ctx);
            return;
        }
        // Paste in the context menu goes where the right click was; from the ribbon and the keys the copies land a step from the originals.
        if matches!(ctl.id, "CanvasPaste" | "ConnectorPaste")
            && let Some((x, y)) = self.ui.edit.context_at()
        {
            self.run_ribbon_params(ctx, command, &json!({"x": x, "y": y}));
            return;
        }
        // Delete acts on the selection and takes the connectors glued to it along (`edit/keys.rs`).
        if command == "shape.delete" {
            self.delete_selection();
            return;
        }
        // Auto Size is a toggle of the page on view: the control sends the state it is not in.
        if ctl.press == Press::AutoSize {
            let on = !self.auto_size_on();
            self.run_ribbon_params(ctx, command, &json!({"on": on}));
            return;
        }
        // The Insert tab's Pages group works on the page on view (`page_tabs.rs`).
        if matches!(ctl.id, "BlankPageInsert" | "BackgroundPageInsert" | "DuplicateThisPage") {
            self.run_pages_control(ctx, ctl.id);
            return;
        }
        // The Design tab's connector and theme commands name the page on view, and a style the connectors selected (`ribbon/design.rs`).
        if matches!(command, "connector.style" | "connector.line_jumps" | "connector.reroute" | "connector.reset" | "theme.apply") {
            match self.design_params(ctl) {
                Ok(params) => self.run_ribbon_params(ctx, command, &params),
                Err(why) => self.set_notice(&why),
            }
            return;
        }
        // So is a check box of a Snap & Glue option (Dynamic Grid): it sends `snap.set` with the option the other way from how the document has it.
        if ctl.kind == Kind::Check && command == "snap.set" {
            match self.snap_box_toggle(ctl.args) {
                Some(params) => self.run_ribbon_params(ctx, command, &params),
                None => self.set_notice(&format!("internal error: `{}` names no Snap & Glue option; please report this bug", ctl.id)),
            }
            return;
        }
        let params = if ctl.args.is_empty() {
            json!({})
        } else {
            match serde_json::from_str::<Value>(ctl.args) {
                Ok(params) => params,
                Err(e) => {
                    self.set_notice(&format!("internal error: the parameters of `{}` are not JSON ({e}); please report this bug", ctl.id));
                    return;
                }
            }
        };
        self.run_ribbon_params(ctx, command, &params);
    }

    /// Runs `command` with `params` for a ribbon control, and tells the person if it fails.
    pub(crate) fn run_ribbon_params(&mut self, ctx: &egui::Context, command: &str, params: &Value) {
        // The page commands of the Design tab's Page Setup group work on the page in view, as the marks of their menus read it: left to the engine they would work on the first page
        // that is not a background, and a background page is set up by showing it.
        let named;
        let params = match self.current_page() {
            Some(page) if matches!(command, "page.size" | "page.orientation" | "page.auto_size") && params.get("page").is_none() => {
                let mut with_page = params.clone();
                if let Some(map) = with_page.as_object_mut() {
                    map.insert("page".to_owned(), json!(page));
                }
                named = with_page;
                &named
            }
            _ => params,
        };
        match self.execute(command, params) {
            // A size or an orientation chosen by hand: the whole page is put in view.
            Ok(_) if matches!(command, "page.size" | "page.orientation") => self.fit_page_to_window(),
            Ok(_) => {}
            Err(e) => self.set_notice(&e.to_string()),
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submenu_is_as_tall_as_the_window_lets_it_be_and_never_less_than_four_rows() {
        assert_eq!(submenu_height(900.0), 876.0);
        assert_eq!(submenu_height(800.0), 776.0);
        assert_eq!(submenu_height(320.0), 296.0, "the app's least window");
        assert_eq!(submenu_height(100.0), SUBMENU_MIN_HEIGHT);
        for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            assert_eq!(submenu_height(odd), SUBMENU_MIN_HEIGHT, "{odd}");
        }
    }
}
