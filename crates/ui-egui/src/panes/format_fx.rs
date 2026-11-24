//! The Format Shape pane's transparency, gradient, pattern, line and shadow controls.
//!
//! **Fill** has four radio buttons, **No fill**, **Solid fill**, **Gradient fill** and **Pattern fill**, and under the one the shapes have its fields: the colour and the
//! transparency (a slider and a number box in percent) for Solid; for Gradient the layout, the angle (a linear one) or the place it starts from (a radial or rectangular
//! one), a bar that shows the gradient with a handle for each stop, **Add stop** and **Remove stop**, and the colour, position and transparency of the stop selected (
//! "transparency belongs to each gradient stop"); for Pattern a grid of the 23 patterns, the colour of the marks, the colour between them and the transparency.
//! **Line** has No line, Solid line and Gradient line, with the colour, transparency, width, compound type, dash type, cap type, rounding and ends.
//! **Effects** has the Shadow group: Presets, Color, Transparency, Size, Blur, Angle and Distance. Reflection, Glow, Soft Edges, Bevel and 3-D Rotation stay quiet.
//!
//! Every control sends one command (`format.shape`, `format.gradient`, `format.shadow`), and the page shows the change at once. **A slider shows its value on the page as
//! it is dragged and is one undo step** (`DiacraftApp::live_send`). The fields of a number take it as typed on Enter, as the weight does (`fields.rs`).
//! **What the controls send is what the commands take**: the pattern swatches are drawn here (our own artwork: lines, grids and dots, never a Microsoft bitmap) and the
//! words of the lists are ours where says the lists are not described.

use diacraft_engine::formatting::{GradientKind, GradientLook, GradientTarget, SHADOW_PRESETS};
use egui::{Align2, CornerRadius, Mesh, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::format::Mix;
use crate::format_fx::PaintKind;
use crate::measure::number_text;
use crate::panes::fields::{Field, LABEL_W, Target, label_column};
use crate::ribbon::controls::Cx;
use crate::ribbon::palette::{WIDTH as PALETTE_WIDTH, colour_menu};
use crate::ribbon::pickers;
use crate::ribbon::spec::{self, Caption, Ctl, Palette};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};

/// The patterns of the fill the pane offers, in order: the number of `FillPattern` and a name in our words (gives only the kind of each).
pub(crate) const PATTERNS: [(u8, &str); 23] = [
    (2, "Diagonal lines, rising"),
    (3, "Grid"),
    (4, "Diagonal lattice"),
    (5, "Diagonal lines, falling"),
    (6, "Horizontal lines"),
    (7, "Vertical lines"),
    (8, "Dense speckle"),
    (9, "Speckle"),
    (10, "Medium speckle"),
    (11, "Light speckle"),
    (12, "Sparse dots"),
    (13, "Heavy horizontal bars"),
    (14, "Heavy vertical bars"),
    (15, "Heavy diagonals, falling"),
    (16, "Heavy diagonals, rising"),
    (17, "Small checks"),
    (18, "Dense lattice"),
    (19, "Thin horizontal lines"),
    (20, "Thin vertical lines"),
    (21, "Thin diagonals, falling"),
    (22, "Thin diagonals, rising"),
    (23, "Fine grid"),
    (24, "Fine lattice"),
];

/// The cap words of `shape.format` and what the pane calls them.
const CAPS: [(&str, &str); 3] = [("round", "Round"), ("flat", "Flat"), ("square", "Square")];
/// The compound words of `shape.format` and what the pane calls them.
const COMPOUNDS: [(&str, &str); 5] = [
    ("single", "Single"),
    ("double", "Double"),
    ("thick_thin", "Thick-thin"),
    ("thin_thick", "Thin-thick"),
    ("triple", "Triple (thin, thick, thin)"),
];
/// The size classes of a line end, 0 to 6, and what the pane calls them (very small, small, medium, large, extra large, jumbo, colossal).
const ARROW_SIZES: [&str; 7] = ["Very small", "Small", "Medium", "Large", "Extra large", "Jumbo", "Colossal"];
/// The layouts of a gradient and the words `shape.gradient` takes for them.
const KINDS: [(&str, &str); 4] = [("linear", "Linear"), ("radial", "Radial"), ("rectangular", "Rectangular"), ("path", "Path")];
/// Where a radial gradient starts, and a rectangular one (which has no edge middles).
const RADIAL_ORIGINS: [(&str, &str); 7] = [
    ("center", "From the center"),
    ("top_left", "From the top left"),
    ("top_right", "From the top right"),
    ("bottom_left", "From the bottom left"),
    ("bottom_right", "From the bottom right"),
    ("top", "From the top"),
    ("bottom", "From the bottom"),
];

/// The names of the shadow presets in the order of [`SHADOW_PRESETS`].
const PRESET_NAMES: [&str; 10] = [
    "No Shadow",
    "Offset Bottom Right",
    "Offset Bottom",
    "Offset Bottom Left",
    "Offset Right",
    "Centered",
    "Offset Left",
    "Offset Top Right",
    "Offset Top",
    "Offset Top Left",
];

/// A gradient control's name: `pane.format.fill.gradient` or `pane.format.line.gradient`, and what follows it.
fn base(target: GradientTarget) -> &'static str {
    match target {
        GradientTarget::Fill => "pane.format.fill.gradient",
        GradientTarget::Line => "pane.format.line.gradient",
    }
}

/// A percentage a field shows: `35%`.
fn percent_text(hundredths: i64) -> String {
    format!("{}%", number_text(hundredths as f64 / 100.0))
}

impl DiacraftApp {
    /// What is under the **Fill** radios for the kind the shapes have.
    pub(super) fn fill_details(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        match self.fill_kind_mix() {
            Mix::One(PaintKind::Solid) => {
                self.colour_swatch(ui, t, "Color", "pane.format.fill.colour", enabled, ("FillColorGallery", "format.shape", "fill", Some("No Fill")));
                self.transparency_row(
                    ui,
                    t,
                    "Transparency",
                    "pane.format.fill.transparency",
                    self.fill_transparency_mix(),
                    enabled,
                    "fill_transparency",
                );
            }
            Mix::One(PaintKind::Gradient) => self.gradient_editor(ui, t, GradientTarget::Fill, enabled),
            Mix::One(PaintKind::Pattern) => self.pattern_editor(ui, t, enabled),
            Mix::Mixed => self.dim_note(ui, t, "The shapes differ: pick a type to give them all one."),
            Mix::One(PaintKind::None) | Mix::Nothing => {}
        }
    }

    /// What is under the **Line** radios for the kind the shapes have: the colour (or the gradient) and the transparency.
    pub(super) fn line_details(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        match self.line_kind_mix() {
            Mix::One(PaintKind::Solid) => {
                self.colour_swatch(ui, t, "Color", "pane.format.line.colour", enabled, ("LineColorGallery", "format.shape", "line", Some("No Line")));
                self.transparency_row(
                    ui,
                    t,
                    "Transparency",
                    "pane.format.line.transparency",
                    self.line_transparency_mix(),
                    enabled,
                    "line_transparency",
                );
            }
            Mix::One(PaintKind::Gradient) => self.gradient_editor(ui, t, GradientTarget::Line, enabled),
            Mix::Mixed => self.dim_note(ui, t, "The shapes differ: pick a type to give them all one."),
            _ => {}
        }
    }

    fn dim_note(&mut self, ui: &mut Ui, t: &Tokens, words: &str) {
        ui.add(egui::Label::new(egui::RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
        ui.add_space(2.0);
    }

    /// The line's cap and compound type, two lists.
    pub(super) fn line_styles(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        let compound = match self.compound_mix() {
            Mix::One(n) => COMPOUNDS.get(usize::from(n)).map(|(_, name)| (*name).to_owned()),
            _ => None,
        };
        let items: Vec<(String, Value)> = COMPOUNDS.iter().map(|(word, name)| ((*name).to_owned(), json!({"compound": word}))).collect();
        self.choice_list(ui, t, "Compound type", "pane.format.line.compound", compound, &items, "format.shape", enabled);
        let cap = match self.cap_mix() {
            Mix::One(n) => CAPS.get(usize::from(n)).map(|(_, name)| (*name).to_owned()),
            _ => None,
        };
        let items: Vec<(String, Value)> = CAPS.iter().map(|(word, name)| ((*name).to_owned(), json!({"line_cap": word}))).collect();
        self.choice_list(ui, t, "Cap type", "pane.format.line.cap", cap, &items, "format.shape", enabled);
    }

    /// Begin Arrow size and End Arrow size: two lists of the size classes of a line end, Medium by default.
    pub(super) fn arrow_sizes(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        for (label, name, mix, key) in [
            ("Begin arrow size", "pane.format.line.begin_size", self.begin_arrow_size_mix(), "begin_arrow_size"),
            ("End arrow size", "pane.format.line.end_size", self.end_arrow_size_mix(), "end_arrow_size"),
        ] {
            let shown = match mix {
                Mix::One(n) => ARROW_SIZES.get(usize::try_from(n).unwrap_or(usize::MAX)).map(|name| (*name).to_owned()),
                _ => None,
            };
            let items: Vec<(String, Value)> = ARROW_SIZES.iter().enumerate().map(|(n, name)| ((*name).to_owned(), json!({ key: n }))).collect();
            self.choice_list(ui, t, label, name, shown, &items, "format.shape", enabled);
        }
    }

    // ---------------------------------------------------------------------------------------------------- transparency

    /// A labelled number box in percent over a slider, which sends `key` of `format.shape` (the fill's or the line's transparency). The box sends on Enter, and the slider
    /// shows its value on the page as it is dragged, in one undo step.
    #[allow(clippy::too_many_arguments)]
    fn transparency_row(&mut self, ui: &mut Ui, t: &Tokens, label: &str, name: &str, mix: Mix<i64>, enabled: bool, key: &'static str) {
        let shown = match mix {
            Mix::One(h) => Some(percent_text(h)),
            _ => None,
        };
        let shapes = self.format_target();
        let target = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
        self.pane_field(ui, t, Field { label, name, shown, enabled, why_not: Some("Nothing is selected"), target });
        let mut value = match mix {
            Mix::One(h) => h as f64 / 100.0,
            _ => 0.0,
        };
        self.percent_slider(ui, t, &format!("{name}.slider"), &mut value, enabled, "format.shape", key, "");
    }

    /// A slider from 0 to 100 percent under a field: `command` is sent with `{param: value}` while it is dragged (`live_send`). `extra` is a JSON object of other parameters
    /// the command is sent with, `""` for none.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn percent_slider(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        name: &str,
        value: &mut f64,
        enabled: bool,
        command: &str,
        param: &str,
        extra: &str,
    ) {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            ui.spacing_mut().slider_width = (ui.available_width() - 12.0).clamp(60.0, 170.0);
            // The rail is the input border's grey and what is passed is the accent, so that both show on the pane's own ground (egui paints the rail in the fill of an idle
            // widget, which the theme makes the pane's white).
            let widgets = &mut ui.visuals_mut().widgets;
            for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.noninteractive] {
                state.bg_fill = t.input_border;
            }
            ui.visuals_mut().selection.bg_fill = t.accent;
            let resp = ui.add_enabled(enabled, egui::Slider::new(value, 0.0..=100.0).show_value(false).step_by(1.0));
            self.note(name.to_owned(), resp.rect, enabled);
            resp.widget_info(|| egui::WidgetInfo::slider(enabled, *value, "Transparency"));
            if resp.changed() && enabled {
                let mut params = serde_json::from_str::<Value>(extra).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
                if let Some(map) = params.as_object_mut() {
                    map.insert(param.to_owned(), json!(*value));
                }
                if resp.dragged() {
                    self.live_send(name, command, &params);
                } else {
                    self.live_end();
                    if let Err(e) = self.execute(command, &params) {
                        self.set_notice(&e.to_string());
                    }
                }
            }
            if resp.drag_stopped() {
                self.live_end();
            }
        });
    }

    // ---------------------------------------------------------------------------------------------------- colours

    /// A colour control: a swatch of the colour the selection has that opens a palette whose choice is sent as `param` of `command`. `home` is the Home tab's control whose
    /// palette it draws again.
    pub(super) fn colour_swatch(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        label: &str,
        name: &str,
        enabled: bool,
        what: (&str, &'static str, &'static str, Option<&'static str>),
    ) {
        let (home, command, key, none) = what;
        let Some(ctl) = spec::home_control(home) else { return };
        let mut control: Ctl = *ctl;
        control.command = Some(command);
        // A colour of the theme and Use Default are for the fill and the line (`format.shape`); the colour of a gradient stop, between the marks of a pattern or of a shadow is a fixed one.
        let themed = command == "format.shape" && matches!(key, "fill" | "line");
        control.palette = Some(Palette {
            key,
            none: none.map(|text| Caption { text, confirmed: false }),
            more: Caption { text: "More Colors...", confirmed: false },
            default: Caption { text: "Use Default", confirmed: false },
            themed,
        });
        let Some(palette) = control.palette else { return };
        let current = pickers::bar_colour(self, &control).unwrap_or(Mix::Nothing);
        ui.horizontal(|ui| {
            label_column(ui, t, label, enabled);
            let (rect, resp) = ui.allocate_exact_size(vec2(56.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_button(&resp, enabled, &format!("{label} of the {key}"));
            paint_button_bg(ui, rect, &resp, enabled, false, t, 3);
            let swatch = rect.shrink2(vec2(4.0, 4.0));
            match &current {
                Mix::One(Some(diacraft_engine::formatting::Rgb(r, g, b))) => {
                    ui.painter().rect_filled(swatch, CornerRadius::same(2), theme::document_colour(*r, *g, *b));
                    ui.painter().rect_stroke(swatch, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
                }
                other => pickers::paint_bar(ui.painter(), swatch, other, t, enabled),
            }
            self.note(name.to_owned(), rect, enabled);
            let resp = resp.on_hover_text(if enabled { "Pick a color" } else { "Not available" });
            let cx = Cx { t: *t, tab: "pane" };
            if enabled {
                egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(PALETTE_WIDTH);
                    colour_menu(self, ui, cx, &control, palette);
                });
            }
        });
    }

    /// The stop colour palette sends `stop_color` (`line_stop_color` for the line's gradient) of `format.gradient`.
    fn stop_colour(&mut self, ui: &mut Ui, t: &Tokens, target: GradientTarget, enabled: bool) {
        let (key, name) = match target {
            GradientTarget::Fill => ("stop_color", "pane.format.fill.gradient.stop.colour"),
            GradientTarget::Line => ("line_stop_color", "pane.format.line.gradient.stop.colour"),
        };
        self.colour_swatch(ui, t, "Color", name, enabled, ("FillColorGallery", "format.gradient", key, None));
    }

    // ---------------------------------------------------------------------------------------------------- lists

    /// A labelled box with a drop-down arrow that opens a list of choices: the row the shapes have is marked, and a press sends `command` with the row's parameters.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn choice_list(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        label: &str,
        name: &str,
        shown: Option<String>,
        items: &[(String, Value)],
        command: &'static str,
        enabled: bool,
    ) {
        ui.horizontal(|ui| {
            label_column(ui, t, label, enabled);
            let width = (ui.available_width() - 6.0).clamp(60.0, 170.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 22.0), if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_button(&resp, enabled, label);
            ui.painter().rect(rect, CornerRadius::same(3), t.input_fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            let clip = ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(rect.right() - 16.0, rect.max.y)));
            clip.text(
                pos2(rect.left() + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                shown.as_deref().unwrap_or(""),
                theme::font(size::LABEL),
                label_colour(t, enabled),
            );
            crate::icons::chevron(ui.painter(), pos2(rect.right() - 8.0, rect.center().y), 2.5, label_colour(t, enabled));
            self.note(name.to_owned(), rect, enabled);
            if enabled {
                egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
                    ui.set_width(width.max(150.0));
                    egui::ScrollArea::vertical().max_height(300.0).auto_shrink([true, true]).show(ui, |ui| {
                        for (n, (text, params)) in items.iter().enumerate() {
                            let marked = shown.as_deref() == Some(text.as_str());
                            let r = ui.add(egui::Button::selectable(marked, egui::RichText::new(text).font(theme::font(size::LABEL)).color(t.text)));
                            self.note(format!("{name}.{n}"), r.rect, true);
                            if r.clicked() {
                                self.pane_send(command, params.clone());
                            }
                        }
                    });
                });
            }
        });
    }

    /// Sends `command` (a window `format.*` command) with `params` for the selection and says why it was refused.
    pub(super) fn pane_send(&mut self, command: &str, params: Value) {
        if let Err(e) = self.execute(command, &params) {
            self.set_notice(&e.to_string());
        }
    }

    // ---------------------------------------------------------------------------------------------------- gradients

    /// The gradient's controls: layout, angle or origin, the bar with its stops, and the stop selected.
    fn gradient_editor(&mut self, ui: &mut Ui, t: &Tokens, target: GradientTarget, enabled: bool) {
        let name = base(target);
        let target_word = target.word();
        let held = match self.gradient_mix(target) {
            Mix::One(Some(g)) => g,
            Mix::Mixed => {
                self.dim_note(ui, t, "The gradients of the shapes differ: change one setting to give them all the same.");
                return;
            }
            _ => return,
        };
        let kind = GradientKind::of_dir(held.dir);
        // The layout.
        let shown = KINDS.iter().find(|(word, _)| *word == kind.word()).map(|(_, text)| (*text).to_owned());
        let items: Vec<(String, Value)> =
            KINDS.iter().map(|(word, text)| ((*text).to_owned(), json!({"target": target_word, "kind": word}))).collect();
        self.choice_list(ui, t, "Type", &format!("{name}.type"), shown, &items, "format.gradient", enabled);
        match kind {
            GradientKind::Linear => {
                let shapes = self.format_target();
                let at = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
                let angle = Some(format!("{} deg", number_text(held.angle)));
                self.pane_field(ui, t, Field { label: "Angle", name: &format!("{name}.angle"), shown: angle, enabled, why_not: None, target: at });
            }
            GradientKind::Radial | GradientKind::Rectangular => {
                let origins: Vec<(&str, &str)> =
                    RADIAL_ORIGINS.iter().filter(|(word, _)| kind == GradientKind::Radial || !matches!(*word, "top" | "bottom")).copied().collect();
                let at = GradientKind::origin_of(held.dir);
                let shown = origins.iter().find(|(word, _)| *word == at).map(|(_, text)| (*text).to_owned());
                let items: Vec<(String, Value)> = origins
                    .iter()
                    .map(|(word, text)| ((*text).to_owned(), json!({"target": target_word, "kind": kind.word(), "origin": word})))
                    .collect();
                self.choice_list(ui, t, "Direction", &format!("{name}.direction"), shown, &items, "format.gradient", enabled);
            }
            GradientKind::Path => {}
        }
        self.gradient_bar(ui, t, target, &held, enabled);
        let selected = self.selected_stop(held.stops.len());
        // Add and remove.
        let (add_on, remove_on) = (enabled && held.stops.len() < diacraft_engine::formatting::MAX_GRADIENT_STOPS, enabled && held.stops.len() > 2);
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            for (label, tag, on, params) in [
                ("Add stop", "add", add_on, json!({"target": target_word, "add_stop": true})),
                ("Remove stop", "remove", remove_on, json!({"target": target_word, "remove_stop": true})),
            ] {
                let r = ui.add_enabled(on, egui::Button::new(egui::RichText::new(label).font(theme::font(size::LABEL))));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, on, label));
                self.note(format!("{name}.{tag}"), r.rect, on);
                if r.clicked() && on {
                    self.pane_send("format.gradient", params);
                }
            }
        });
        // The stop selected.
        let Some(stop) = held.stops.get(selected).copied() else { return };
        self.stop_colour(ui, t, target, enabled);
        let shapes = self.format_target();
        let at = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
        let position = Some(format!("{}%", number_text(stop.position * 100.0)));
        self.pane_field(
            ui,
            t,
            Field { label: "Position", name: &format!("{name}.stop.position"), shown: position, enabled, why_not: None, target: at },
        );
        let at = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
        let trans = Some(format!("{}%", number_text(stop.transparency)));
        self.pane_field(
            ui,
            t,
            Field { label: "Transparency", name: &format!("{name}.stop.transparency"), shown: trans, enabled, why_not: None, target: at },
        );
        let mut value = stop.transparency;
        let extra = json!({"target": target_word, "stop": selected}).to_string();
        self.percent_slider(ui, t, &format!("{name}.stop.transparency.slider"), &mut value, enabled, "format.gradient", "stop_transparency", &extra);
        if target == GradientTarget::Fill {
            let on = held.rotate_with_shape;
            let r = ui.add_enabled(
                enabled,
                egui::Checkbox::new(
                    &mut { on },
                    egui::RichText::new("Rotate gradient with shape").font(theme::font(size::LABEL)).color(label_colour(t, enabled)),
                ),
            );
            widgets::name_check(&r, enabled, on, "Rotate gradient with shape");
            self.note(format!("{name}.rotate"), r.rect, enabled);
            if r.clicked() && enabled {
                self.pane_send("format.gradient", json!({"rotate_with_shape": !on}));
            }
        }
    }

    /// The bar: the gradient as it is painted from the first stop to the last, with a handle under each stop. A press on a handle selects the stop.
    fn gradient_bar(&mut self, ui: &mut Ui, t: &Tokens, target: GradientTarget, g: &GradientLook, enabled: bool) {
        let name = base(target);
        let selected = self.selected_stop(g.stops.len());
        let width = (ui.available_width() - 14.0).max(60.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
        self.note(format!("{name}.bar"), rect, enabled);
        let bar = Rect::from_min_size(rect.min + vec2(6.0, 2.0), vec2(width - 12.0, 20.0));
        paint_bar(ui.painter(), bar, g, t, enabled);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, enabled, "Gradient"));
        for (n, stop) in g.stops.iter().enumerate() {
            let x = bar.left() + bar.width() * stop.position as f32;
            let handle = Rect::from_center_size(pos2(x, bar.bottom() + 9.0), vec2(14.0, 14.0));
            let id = ui.scope_id().with((name, "stop", n));
            let r = ui.interact(handle, id, if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_toggle(&r, enabled, n == selected, &format!("Stop {}", n + 1));
            let painter = ui.painter();
            let triangle = vec![pos2(x, bar.bottom() + 1.0), pos2(x - 6.0, bar.bottom() + 8.0), pos2(x + 6.0, bar.bottom() + 8.0)];
            painter.add(egui::Shape::convex_polygon(
                triangle,
                t.input_fill,
                Stroke::new(if n == selected { 2.0 } else { 1.0 }, if n == selected { t.accent } else { t.input_border }),
            ));
            let swatch = Rect::from_center_size(pos2(x, bar.bottom() + 14.5), vec2(11.0, 8.0));
            painter.rect_filled(swatch, CornerRadius::same(1), theme::document_colour(stop.colour.0, stop.colour.1, stop.colour.2));
            painter.rect_stroke(
                swatch,
                CornerRadius::same(1),
                Stroke::new(if n == selected { 2.0 } else { 1.0 }, if n == selected { t.accent } else { t.input_border }),
                StrokeKind::Outside,
            );
            self.note(format!("{name}.stop.{n}"), handle, enabled);
            if r.clicked() && enabled {
                self.ui.panes.format.fx.stop = n;
            }
            r.on_hover_text(format!("Stop {}: {}%", n + 1, number_text(stop.position * 100.0)));
        }
    }

    // ---------------------------------------------------------------------------------------------------- patterns

    /// The pattern fill's controls: the grid of patterns, the colour of the marks and the colour between them, and the transparency.
    fn pattern_editor(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        let now = match self.pattern_fill_mix() {
            Mix::One(n) => Some(n),
            _ => None,
        };
        let fore = match self.fill_mix() {
            Mix::One(Some(rgb)) => rgb,
            _ => diacraft_engine::formatting::Rgb(0, 0, 0),
        };
        let back = match self.background_mix() {
            Mix::One(Some(rgb)) => rgb,
            _ => diacraft_engine::formatting::Rgb(255, 255, 255),
        };
        let columns = 6;
        let cell = 30.0;
        for row in PATTERNS.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (number, label) in row {
                    let (rect, resp) = ui.allocate_exact_size(vec2(cell, cell), if enabled { Sense::click() } else { Sense::hover() });
                    widgets::name_toggle(&resp, enabled, now == Some(u32::from(*number)), label);
                    let marked = now == Some(u32::from(*number));
                    let painter = ui.painter();
                    paint_pattern(painter, rect.shrink(1.0), *number, fore, back, t, enabled);
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(2),
                        Stroke::new(if marked { 2.0 } else { 1.0 }, if marked { t.accent } else { t.input_border }),
                        StrokeKind::Inside,
                    );
                    self.note(format!("pane.format.fill.pattern.{number}"), rect, enabled);
                    let resp = resp.on_hover_text(*label);
                    if resp.clicked() && enabled {
                        self.ui.panes.format.fx.pattern = *number;
                        self.pane_send("format.shape", json!({"fill_pattern": number}));
                    }
                }
            });
            ui.add_space(2.0);
        }
        self.colour_swatch(ui, t, "Foreground", "pane.format.fill.colour", enabled, ("FillColorGallery", "format.shape", "fill", None));
        self.colour_swatch(
            ui,
            t,
            "Background",
            "pane.format.fill.background",
            enabled,
            ("FillColorGallery", "format.shape", "fill_background", None),
        );
        self.transparency_row(ui, t, "Transparency", "pane.format.fill.transparency", self.fill_transparency_mix(), enabled, "fill_transparency");
    }

    // ---------------------------------------------------------------------------------------------------- shadow

    /// The **Shadow** group of the Effects tab: Presets, Color, Transparency, Size, Blur, Angle and Distance. The fields are quiet while the shapes have no shadow.
    pub(super) fn shadow_section(&mut self, ui: &mut Ui, t: &Tokens, selected: bool) {
        let mix = self.shadow_mix();
        let has = matches!(mix, Mix::One(Some(_)) | Mix::Mixed);
        let items: Vec<(String, Value)> =
            SHADOW_PRESETS.iter().zip(PRESET_NAMES).map(|(word, name)| (name.to_owned(), json!({"preset": word}))).collect();
        let shown = match &mix {
            Mix::One(None) => Some("No Shadow".to_owned()),
            Mix::One(Some(s)) => Some(preset_name(s).to_owned()),
            Mix::Mixed | Mix::Nothing => None,
        };
        self.choice_list(ui, t, "Presets", "pane.format.shadow.presets", shown, &items, "format.shadow", selected);
        let enabled = selected && has;
        let shadow = match &mix {
            Mix::One(Some(s)) => Some(s.clone()),
            _ => None,
        };
        self.colour_swatch(ui, t, "Color", "pane.format.shadow.colour", enabled, ("FillColorGallery", "format.shadow", "shadow_color", None));
        let shapes = self.format_target();
        let field = |app: &mut DiacraftApp, ui: &mut Ui, label: &str, name: &str, shown: Option<String>| {
            let target = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
            app.pane_field(ui, t, Field { label, name, shown, enabled, why_not: Some("The shapes have no shadow: pick a preset"), target });
        };
        field(self, ui, "Transparency", "pane.format.shadow.transparency", shadow.as_ref().map(|s| format!("{}%", number_text(s.transparency))));
        let mut value = shadow.as_ref().map_or(0.0, |s| s.transparency);
        self.percent_slider(ui, t, "pane.format.shadow.transparency.slider", &mut value, enabled, "format.shadow", "transparency", "");
        field(self, ui, "Size", "pane.format.shadow.size", shadow.as_ref().map(|s| format!("{}%", number_text(s.size))));
        field(self, ui, "Blur", "pane.format.shadow.blur", shadow.as_ref().map(|s| format!("{} pt", number_text(s.blur))));
        field(self, ui, "Angle", "pane.format.shadow.angle", shadow.as_ref().map(|s| format!("{} deg", number_text(s.angle))));
        field(self, ui, "Distance", "pane.format.shadow.distance", shadow.as_ref().map(|s| format!("{} pt", number_text(s.distance))));
    }
}

/// The name of the preset a shadow is, or `Custom`: the starting shadow in a direction (`shape.shadow`'s presets), whatever the person has not changed since.
fn preset_name(s: &diacraft_engine::formatting::ShadowLook) -> &'static str {
    let black = s.colour == diacraft_engine::formatting::Rgb(0, 0, 0);
    let plain = black && (s.transparency - 60.0).abs() < 0.01 && (s.size - 100.0).abs() < 0.01 && s.kind == 1;
    for (word, name) in SHADOW_PRESETS.iter().zip(PRESET_NAMES).skip(1) {
        let Some(preset) = diacraft_engine::formatting::shadow_preset(word) else { continue };
        let (angle, distance, blur) = (preset.angle.unwrap_or(0.0), preset.distance.unwrap_or(0.0), preset.blur.unwrap_or(0.0));
        let same_angle = distance == 0.0 && s.distance < 0.01 || (s.angle - angle).abs() < 0.01 && (s.distance - distance).abs() < 0.01;
        if plain && same_angle && (s.blur - blur).abs() < 0.01 {
            return name;
        }
    }
    "Custom"
}

/// Paints the gradient as it runs from the first stop to the last along the bar (a stop's transparency shows the bar's ground through it).
pub(crate) fn paint_bar(painter: &Painter, rect: Rect, g: &GradientLook, t: &Tokens, enabled: bool) {
    painter.rect_filled(rect, CornerRadius::same(2), t.input_fill);
    if enabled && !g.stops.is_empty() {
        let mut mesh = Mesh::default();
        let steps = 48_usize;
        for i in 0..=steps {
            let at = i as f64 / steps as f64;
            let (r, gc, b, a) = crate::format_fx::gradient_colour_at(g, at);
            let x = rect.left() + rect.width() * at as f32;
            let colour = theme::document_colour_alpha(r, gc, b, a);
            mesh.colored_vertex(pos2(x, rect.top()), colour);
            mesh.colored_vertex(pos2(x, rect.bottom()), colour);
            if i > 0 {
                let k = (i * 2) as u32;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k, k + 1);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
}

/// Paints the swatch of the pattern `number` (2 to 24) in `rect`: the colour between the marks, then the marks (our own artwork: a
/// kind of fill and no Microsoft bitmap).
pub(crate) fn paint_pattern(
    painter: &Painter,
    rect: Rect,
    number: u8,
    fore: diacraft_engine::formatting::Rgb,
    back: diacraft_engine::formatting::Rgb,
    t: &Tokens,
    enabled: bool,
) {
    let ground = if enabled { theme::document_colour(back.0, back.1, back.2) } else { t.icon_fill_disabled };
    let ink = if enabled { theme::document_colour(fore.0, fore.1, fore.2) } else { t.icon_ink_disabled };
    let clip = painter.with_clip_rect(rect);
    clip.rect_filled(rect, CornerRadius::ZERO, ground);
    // (what it draws, the step between marks in points, the weight of a mark in points)
    let hatch = |kind: Lines, step: f32, weight: f32| paint_lines(&clip, rect, kind, step, weight, ink);
    match number {
        2 => hatch(Lines::Rising, 6.0, 1.0),
        3 => hatch(Lines::Grid, 6.0, 1.0),
        4 => hatch(Lines::Lattice, 6.0, 1.0),
        5 => hatch(Lines::Falling, 6.0, 1.0),
        6 => hatch(Lines::Horizontal, 5.0, 1.0),
        7 => hatch(Lines::Vertical, 5.0, 1.0),
        8..=12 => {
            // Speckle: the lower the number the fewer the dots (8 is the densest).
            let keep = [8, 6, 4, 2, 1][usize::from(number - 8)];
            let mut n = 0_u32;
            let mut y = rect.top() + 1.0;
            while y < rect.bottom() {
                let mut x = rect.left() + 1.0;
                while x < rect.right() {
                    n = n.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    if (n >> 16) % 10 < keep {
                        clip.rect_filled(Rect::from_min_size(pos2(x, y), vec2(1.5, 1.5)), CornerRadius::ZERO, ink);
                    }
                    x += 2.5;
                }
                y += 2.5;
            }
        }
        13 => hatch(Lines::Horizontal, 5.0, 3.0),
        14 => hatch(Lines::Vertical, 5.0, 3.0),
        15 => hatch(Lines::Falling, 5.0, 3.0),
        16 => hatch(Lines::Rising, 5.0, 3.0),
        17 => {
            let cell = 4.0;
            let mut row = 0;
            let mut y = rect.top();
            while y < rect.bottom() {
                let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { cell };
                while x < rect.right() {
                    clip.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cell, cell)), CornerRadius::ZERO, ink);
                    x += cell * 2.0;
                }
                y += cell;
                row += 1;
            }
        }
        18 => hatch(Lines::Grid, 3.0, 1.0),
        19 => hatch(Lines::Horizontal, 3.0, 1.0),
        20 => hatch(Lines::Vertical, 3.0, 1.0),
        21 => hatch(Lines::Falling, 3.0, 1.0),
        22 => hatch(Lines::Rising, 3.0, 1.0),
        23 => hatch(Lines::Grid, 3.0, 0.5),
        _ => hatch(Lines::Lattice, 3.0, 0.5),
    }
}

/// The lines a hatch is made of.
#[derive(Clone, Copy)]
enum Lines {
    Horizontal,
    Vertical,
    Rising,
    Falling,
    Grid,
    Lattice,
}

/// Lines in `rect` every `step` points, `weight` points thick.
fn paint_lines(painter: &Painter, rect: Rect, kind: Lines, step: f32, weight: f32, ink: egui::Color32) {
    let stroke = Stroke::new(weight, ink);
    let (w, h) = (rect.width(), rect.height());
    let across = |painter: &Painter, rising: bool| {
        // Diagonals at 45 degrees: a line for each step along the longer side, beyond the corners.
        let mut d = -h;
        while d < w {
            let (a, b) = if rising {
                (pos2(rect.left() + d, rect.bottom()), pos2(rect.left() + d + h, rect.top()))
            } else {
                (pos2(rect.left() + d, rect.top()), pos2(rect.left() + d + h, rect.bottom()))
            };
            painter.line_segment([a, b], stroke);
            d += step;
        }
    };
    let (rows, columns) = match kind {
        Lines::Horizontal | Lines::Grid => (true, matches!(kind, Lines::Grid)),
        Lines::Vertical => (false, true),
        _ => (false, false),
    };
    if rows {
        let mut y = rect.top() + step / 2.0;
        while y < rect.bottom() {
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
            y += step;
        }
    }
    if columns {
        let mut x = rect.left() + step / 2.0;
        while x < rect.right() {
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
            x += step;
        }
    }
    match kind {
        Lines::Rising => across(painter, true),
        Lines::Falling => across(painter, false),
        Lines::Lattice => {
            across(painter, true);
            across(painter, false);
        }
        _ => {}
    }
}
