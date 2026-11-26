//! The Layout and Routing Spacing dialog, opened by the **Spacing...** button of the Layout and Routing tab: four groups of a Horizontal and a
//! Vertical length, **Defaults**, OK and Cancel.
//!
//! The boxes start from the tab's draft (which starts from `page.routing`). **OK puts them in the tab's draft** (the Page Setup dialog's own OK sends them, as `page.set_routing`'s four
//! groups, one undo step with the rest); Cancel and Escape drop what was typed; the dialog is a modal over the Page Setup dialog, so Escape and Enter are its own while it is open.
//! **Defaults** fills both boxes of each group with the root style's values: 7.5, 5, 2.5 and 2.5 mm where the page's scale is given in a metric
//! unit, 3/8, 1/4, 1/8 and 1/8 inch otherwise, in the order of the groups. They are lengths on paper, which is what the boxes hold, so on a scaled page the command writes them
//! as the paper length times the scale. A box takes a number in the unit shown, or a length with a unit (`1 cm`: `measure.rs`). **A box the person changed that is no length more than 0
//! is told in words and the dialog stays; a box that still says what it said is not looked at** (a file may hold a gap of 0).

use egui::{Context, Id, Key, Modifiers, RichText, Ui};

use super::routing::{GROUP_KEYS, GROUP_NAMES, GROUPS, Routing, Seen, Shown, length_words};
use super::{field, heading, label};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The dialog while it is open.
pub(super) struct Spacing {
    /// The boxes as typed, and as they were when the dialog opened.
    pub(super) boxes: [(String, String); GROUPS],
    opened: [(String, String); GROUPS],
    error: Option<String>,
}

/// What a frame of the dialog was asked.
#[derive(Default)]
struct Pressed {
    ok: bool,
    cancel: bool,
}

impl Spacing {
    /// The dialog on the tab's four groups as the draft has them now.
    pub(super) fn open(routing: &Routing) -> Spacing {
        Spacing { boxes: routing.spacing.clone(), opened: routing.spacing.clone(), error: None }
    }

    /// Both boxes of every group at the root's value, in the unit shown.
    fn defaults(&mut self, shown: Shown) {
        for (group, pair) in self.boxes.iter_mut().enumerate() {
            let text = shown.root(group);
            *pair = (text.clone(), text);
        }
    }

    /// The boxes with each one the person changed written as the box shows a length (`1 cm` is `0.3937` in inches).
    fn written(mut self, routing: &Routing) -> [(String, String); GROUPS] {
        for (now, was) in self.boxes.iter_mut().zip(&self.opened) {
            for (text, before) in [(&mut now.0, &was.0), (&mut now.1, &was.1)] {
                if text.trim() != before.trim()
                    && let Some(inches) = routing.length(text)
                {
                    *text = routing.shown.text(inches);
                }
            }
        }
        self.boxes
    }

    /// The words that refuse a box the person changed that is no length more than 0, if there is one.
    fn refusal(&self, routing: &Routing) -> Option<String> {
        for (group, (now, was)) in self.boxes.iter().zip(&self.opened).enumerate() {
            for (axis, (text, before)) in [(&now.0, &was.0), (&now.1, &was.1)].into_iter().enumerate() {
                if text.trim() != before.trim() && routing.length(text).is_none() {
                    return Some(length_words(group, axis));
                }
            }
        }
        None
    }
}

/// The groups and the buttons of the dialog.
fn content(ui: &mut Ui, t: &Tokens, state: &mut Spacing, shown: Shown, max_height: f32, seen: &mut Seen) -> (bool, Pressed) {
    ui.label(RichText::new("Layout and Routing Spacing").font(theme::font(size::TAB + 1.0)).color(t.text));
    ui.add_space(6.0);
    egui::ScrollArea::vertical().id_salt("routing_spacing_content").max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
        for (group, name) in GROUP_NAMES.iter().enumerate() {
            heading(ui, t, name);
            let key = GROUP_KEYS.get(group).copied().unwrap_or("group");
            if let Some((x, y)) = state.boxes.get_mut(group) {
                ui.horizontal_wrapped(|ui| {
                    label(ui, t, "Horizontal");
                    field(ui, x, &format!("routingspacing.{key}.x"), true, seen);
                    label(ui, t, "Vertical");
                    field(ui, y, &format!("routingspacing.{key}.y"), true, seen);
                    label(ui, t, shown.word());
                });
            }
            ui.add_space(4.0);
        }
    });
    ui.add_space(4.0);
    if let Some(error) = &state.error {
        ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
        ui.add_space(4.0);
    }
    let mut pressed = Pressed::default();
    let mut defaults = false;
    ui.horizontal_wrapped(|ui| {
        for (words, key) in [("Defaults", "routingspacing.defaults"), ("OK", "routingspacing.ok"), ("Cancel", "routingspacing.cancel")] {
            let button = ui.add(egui::Button::new(RichText::new(words).font(theme::font(size::LABEL))).min_size(egui::vec2(84.0, 26.0)));
            seen.push((key.to_owned(), button.rect, true));
            if button.clicked() {
                match key {
                    "routingspacing.defaults" => defaults = true,
                    "routingspacing.ok" => pressed.ok = true,
                    _ => pressed.cancel = true,
                }
            }
        }
    });
    (defaults, pressed)
}

impl DiacraftApp {
    /// The Spacing dialog over the Page Setup dialog, while the tab holds it open. It is drawn after the Page Setup dialog's own modal, so it is the one on top, and it takes Escape and
    /// Enter (the Page Setup dialog leaves them while this is open).
    pub(super) fn routing_spacing_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some((mut state, shown)) =
            self.ui.page_setup.dialog.as_mut().and_then(|d| d.routing.as_mut()).and_then(|r| r.dialog.take().map(|state| (state, r.shown)))
        else {
            return;
        };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(250.0, 420.0);
        let max_height = (screen.height() - 190.0).clamp(60.0, 400.0);
        let typing = ctx.text_edit_focused();
        let mut seen: Seen = Vec::new();
        let mut pressed = Pressed::default();
        let mut defaults = false;
        let modal = egui::Modal::new(Id::unique("routing_spacing_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            (defaults, pressed) = content(ui, &t, &mut state, shown, max_height, &mut seen);
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            pressed.cancel = true;
        }
        if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            pressed.ok = true;
        }
        if defaults {
            state.defaults(shown);
            state.error = None;
        }
        let Some(routing) = self.ui.page_setup.dialog.as_mut().and_then(|d| d.routing.as_mut()) else { return };
        if pressed.cancel {
            return;
        }
        if pressed.ok {
            match state.refusal(routing) {
                None => {
                    routing.spacing = state.written(routing);
                    return;
                }
                Some(words) => state.error = Some(words),
            }
        }
        routing.dialog = Some(state);
    }
}
