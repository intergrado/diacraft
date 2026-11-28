//! A menu that is taller than its window scrolls inside it, and follows the keyboard's focus.
//!
//! egui moves a popup into the window only as far as the window's edge allows: a menu taller than the window had its top at the top of the window and its last rows below the
//! bottom edge, out of the pointer's reach (the Line menu with its colours is 536 points tall, and the least window the app takes is 320). [`tall_menu`] gives the menu the height a
//! list inside a menu has (`submenu_height`: the window's, less a margin, never less than four rows): a menu that fits is as tall as its rows and is moved up into the window, as it was;
//! one that does not is that tall and scrolls, with a bar that is always drawn (egui's own shows only under the pointer, and a person saw no sign of the rows below).
//!
//! **The arrow keys follow.** egui moves the focus to the next row in the menu's layer (`MenuState::handle_keyboard`) and scrolls to none: the focus went to rows past the edge. [`follow_focus`]
//! brings the row that has the focus into view, once for each row the focus comes to, so a wheel or a bar that scrolled away is not taken back every frame.

use egui::{Id, ScrollArea, Ui, style::ScrollStyle};

use crate::ribbon::controls::submenu_height;
use crate::theme::Tokens;

/// Draws `rows` in a menu `width` points wide (the width of its rows, with no room for a scroll bar), as tall as the window lets a menu be and scrolling past that. Call it from the
/// popup's own `ui`, with the rows in the closure and no `set_width` of their own: the closure's width is `width` whether or not the bar is drawn.
///
/// The room for the bar is added to the width of a menu that scrolls, so its rows are as wide as they were (a row whose label fills the menu would elide it otherwise). Whether the menu
/// scrolls is what its last frame found: a frame is asked for when that changes, once.
pub(crate) fn tall_menu(ui: &mut Ui, t: &Tokens, width: f32, rows: impl FnOnce(&mut Ui)) {
    let key = ui.scope_id().with("tall-menu");
    let scrolling = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let bar = ScrollStyle::solid().allocated_width();
    ui.set_width(width + if scrolling { bar } else { 0.0 });
    // The bar is drawn in the colours of the theme and always, and the rows are drawn as they were: the style the bar needs is put back inside.
    let (scroll, handle) = (ui.spacing().scroll, ui.visuals().widgets.inactive.bg_fill);
    ui.style_mut().spacing.scroll = ScrollStyle::solid();
    ui.visuals_mut().widgets.inactive.bg_fill = t.input_border;
    let height = submenu_height(ui.ctx().content_rect().height());
    let shown = ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([true, true]).show(ui, |ui| {
        ui.style_mut().spacing.scroll = scroll;
        ui.visuals_mut().widgets.inactive.bg_fill = handle;
        rows(ui);
        follow_focus(ui);
    });
    let overflows = shown.content_size.y > shown.inner_rect.height() + 0.5;
    if overflows != scrolling {
        ui.data_mut(|d| d.insert_temp(key, overflows));
        ui.request_repaint();
    }
}

/// Scrolls the scroll area `ui` is in to the widget that has the keyboard's focus, when it is a widget of this menu's own layer and the focus has come to it since the last time: the
/// minimum scroll that brings it into view. Call it after the rows are drawn, so that every row has its place of this frame.
pub(crate) fn follow_focus(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some(focused) = ctx.memory(|m| m.focused()) else { return };
    let key = ui.scope_id().with("focus-followed");
    if ctx.data(|d| d.get_temp::<Id>(key)) == Some(focused) {
        return;
    }
    let Some(place) = ctx.read_response(focused).filter(|r| r.layer_id == ui.layer_id()) else { return };
    ctx.data_mut(|d| d.insert_temp(key, focused));
    ui.scroll_to_rect(place.rect, None);
}
