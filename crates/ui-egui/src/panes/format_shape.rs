//! The Format Shape pane ("a side pane opened from a shape's menu or F3 has a Fill & Line tab and an Effects tab, with live preview on
//! the selection").
//!
//! **Fill & Line** shows the selection's state (`shape.formatting`, through the window's `FormatView`: a control shows a value when every shape has
//! it and nothing when they differ) and **changes it at once**, one `format.shape` for each change, so the picture of the page is the live preview and Undo
//! takes one change back. Fill is none or solid with a colour; Line is none or solid with a colour, a weight in points (steps of a quarter point, the
//! inventory's "exact width in Format Shape in steps of 0.25 pt"), a dash type, a begin and an end arrow, and the rounding of the corners. The colours,
//! dashes and arrows are the Home tab's own controls drawn again (`ribbon/controls.rs`'s `menu_row`, `pickers::colour_menu`), so a press here and a press there send
//! the same command. **The fill and the line have their transparency, a gradient and (a fill) a pattern**, and **the Effects tab has the Shadow group**
//! (`format_fx.rs`). **What no command sets is drawn quiet**: reflection, glow, soft edges, bevel and 3-D. The two tabs are **view state** of the pane,
//! like the tabs of a space in the dock: they do not change the drawing and send no command.
//!
//! **A weight or a rounding typed in a box goes to the shapes the box showed when it took the focus**, in one `shape.format` with their ids, whatever is
//! selected when it lets go of the focus (`fields.rs`); with more shapes than one command takes (4,096) it is refused in words.

use diacraft_units::Unit;
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::format::Mix;
use crate::format_fx::PaintKind;
use crate::measure::{length_text, number_text, parse_length};
use crate::panes::fields::{Field, LABEL_W, Target, Typed, label_column};
use crate::panes::size_position::MAX_SHAPES;
use crate::ribbon::controls::{Cx, menu_row, menu_width};
use crate::ribbon::spec::{self, BEGIN_ARROW_ITEMS, Ctl, DASH_ITEMS, END_ARROW_ITEMS};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour};

/// The tabs of the pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormatTab {
    #[default]
    FillLine,
    Effects,
    /// The Alt Text section (`alt_text.rs`): a third tab, as the area A specification pictures the pane.
    AltText,
}

impl FormatTab {
    /// The tab's name, as the pane shows it and as `format.options` takes it.
    pub const fn name(self) -> &'static str {
        match self {
            FormatTab::FillLine => "Fill & Line",
            FormatTab::Effects => "Effects",
            FormatTab::AltText => "Alt Text",
        }
    }

    /// The tab named `name`, in any case. **Size & Properties** is the name the Microsoft page gives the tab that holds the Alt Text section: it opens this tab, which holds
    /// that section alone (Size & Position is a pane of its own here).
    pub fn from_name(name: &str) -> Option<FormatTab> {
        let name = name.trim();
        if name.eq_ignore_ascii_case("Size & Properties") {
            return Some(FormatTab::AltText);
        }
        [FormatTab::FillLine, FormatTab::Effects, FormatTab::AltText].into_iter().find(|t| t.name().eq_ignore_ascii_case(name))
    }
}

/// What the pane remembers: the tab, and the colours the Solid fill and Solid line choices start from when the shape has none (the last colour any shape had).
#[derive(Debug)]
pub struct FormatState {
    pub tab: FormatTab,
    solid_fill: String,
    solid_line: String,
    /// What the pane remembers for its gradient, pattern and transparency controls (`format_fx.rs`).
    pub(crate) fx: crate::format_fx::FxState,
}

impl Default for FormatState {
    fn default() -> Self {
        // **Ours** (no specification says what Solid fill starts as): a light blue for a fill and the dark grey a line usually is.
        FormatState {
            tab: FormatTab::FillLine,
            solid_fill: "#cfe2f3".to_owned(),
            solid_line: "#404040".to_owned(),
            fx: crate::format_fx::FxState::default(),
        }
    }
}

/// The most a weight may be (points) and a rounding (paper inches): the engine's own bounds (`cmd/format.rs`).
const MAX_WEIGHT_POINTS: f64 = 1_000.0;
const MAX_ROUNDING_PAPER_INCHES: f64 = 100.0;

/// The weight step of the weight's buttons, in points.
const WEIGHT_STEP: f64 = 0.25;

/// The effects the Effects tab lists after the Shadow group: none has a command yet (their rows keep the place they had in the list of six).
const EFFECTS: [&str; 5] = ["Reflection", "Glow", "Soft Edges", "Bevel", "3-D Rotation"];

impl DiacraftApp {
    /// `format.options`: opens the Format Shape pane on a tab (Fill & Line when none is named): what Fill Options, Line Options and More Arrows do.
    pub(crate) fn format_options_command(&mut self, tab: Option<&str>) -> diacraft_engine::Result<()> {
        let chosen = match tab {
            None => FormatTab::FillLine,
            Some(name) => FormatTab::from_name(name).ok_or_else(|| diacraft_engine::EngineError::BadParams {
                cmd: "format.options".to_owned(),
                msg: format!("no tab `{}`; the tabs are: Fill & Line, Effects, Alt Text", diacraft_engine::shown(name, 32)),
            })?,
        };
        self.ui.panes.format.tab = chosen;
        self.pane_command("format_shape", Some(true))
    }

    pub(super) fn format_shape_body(&mut self, ui: &mut Ui, t: &Tokens) {
        self.refresh_format_view();
        // The colours the Solid choices start from are the last the selection had.
        if let Mix::One(Some(rgb)) = self.fill_mix() {
            self.ui.panes.format.solid_fill = rgb.hex();
        }
        if let Mix::One(Some(rgb)) = self.line_mix() {
            self.ui.panes.format.solid_line = rgb.hex();
        }
        ui.horizontal_wrapped(|ui| {
            for (tab, label, name) in [
                (FormatTab::FillLine, "Fill & Line", "pane.format.tab.fill_line"),
                (FormatTab::Effects, "Effects", "pane.format.tab.effects"),
                (FormatTab::AltText, "Alt Text", "pane.format.tab.alt_text"),
            ] {
                let selected = self.ui.panes.format.tab == tab;
                let r = ui.add(egui::Button::selectable(selected, egui::RichText::new(label).font(theme::font(size::LABEL)).color(t.text)));
                self.note(name, r.rect, true);
                if r.clicked() {
                    self.ui.panes.format.tab = tab;
                }
            }
        });
        ui.add_space(6.0);
        // What is selected on another page than the one on view is not shown, and nothing of it can be formatted from here (`selection_on_view`).
        let selected = self.selection_here().is_some();
        let enabled = selected && self.command_enabled("format.shape").is_ok();
        // (The Alt Text tab says in its own words what it needs.)
        if !selected && self.ui.panes.format.tab != FormatTab::AltText {
            ui.add(egui::Label::new(egui::RichText::new("Select a shape to format it.").font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            ui.add_space(4.0);
        }
        match self.ui.panes.format.tab {
            FormatTab::FillLine => {
                self.fill_section(ui, t, enabled);
                ui.add_space(10.0);
                self.line_section(ui, t, enabled);
            }
            FormatTab::Effects => self.effects_section(ui, t, enabled),
            FormatTab::AltText => self.alt_text_section(ui, t),
        }
    }

    fn section_heading(ui: &mut Ui, t: &Tokens, words: &str) {
        ui.label(egui::RichText::new(words).font(theme::font(size::TAB)).color(t.text));
        ui.add_space(2.0);
    }

    /// A row of a pane that has no command behind it: drawn quiet, with the words.
    fn quiet_pane_row(&mut self, ui: &mut Ui, t: &Tokens, label: &str, name: &str) {
        let r = ui.add(egui::Label::new(egui::RichText::new(label).font(theme::font(size::LABEL)).color(t.text_disabled)).sense(Sense::hover()));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, label));
        self.note(name.to_owned(), r.rect, false);
        r.on_hover_text("No command sets this yet");
    }

    fn fill_section(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        Self::section_heading(ui, t, "Fill");
        let kind = self.fill_kind_mix();
        let is = |k: PaintKind| matches!(kind, Mix::One(now) if now == k);
        let mut send: Option<(&str, serde_json::Value)> = None;
        if self.pane_radio(ui, t, "No fill", "pane.format.fill.none", is(PaintKind::None), enabled) && !is(PaintKind::None) {
            send = Some(("format.shape", json!({"fill": "none"})));
        }
        if self.pane_radio(ui, t, "Solid fill", "pane.format.fill.solid", is(PaintKind::Solid), enabled) && !is(PaintKind::Solid) {
            // A pattern or a legacy gradient that the shape has is put away too: the colour alone would leave it.
            send = Some(("format.shape", json!({"fill": self.ui.panes.format.solid_fill.clone(), "fill_pattern": 1})));
        }
        if self.pane_radio(ui, t, "Gradient fill", "pane.format.fill.gradient", is(PaintKind::Gradient), enabled) && !is(PaintKind::Gradient) {
            send = Some(("format.gradient", json!({"on": true})));
        }
        if self.pane_radio(ui, t, "Pattern fill", "pane.format.fill.pattern", is(PaintKind::Pattern), enabled) && !is(PaintKind::Pattern) {
            let fx = &self.ui.panes.format.fx;
            let ink = if matches!(kind, Mix::One(PaintKind::None)) || !matches!(self.fill_mix(), Mix::One(Some(_))) {
                self.ui.panes.format.solid_fill.clone()
            } else if let Mix::One(Some(rgb)) = self.fill_mix() {
                rgb.hex()
            } else {
                self.ui.panes.format.solid_fill.clone()
            };
            send = Some(("format.shape", json!({"fill": ink, "fill_pattern": fx.pattern, "fill_background": fx.background.clone()})));
        }
        if let Some((command, params)) = send {
            self.pane_send(command, params);
        }
        self.fill_details(ui, t, enabled);
    }

    fn line_section(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        Self::section_heading(ui, t, "Line");
        let kind = self.line_kind_mix();
        let is = |k: PaintKind| matches!(kind, Mix::One(now) if now == k);
        let mut send: Option<(&str, serde_json::Value)> = None;
        if self.pane_radio(ui, t, "No line", "pane.format.line.none", is(PaintKind::None), enabled) && !is(PaintKind::None) {
            send = Some(("format.shape", json!({"line": "none"})));
        }
        if self.pane_radio(ui, t, "Solid line", "pane.format.line.solid", is(PaintKind::Solid), enabled) && !is(PaintKind::Solid) {
            send = Some(("format.shape", json!({"line": self.ui.panes.format.solid_line.clone()})));
        }
        if self.pane_radio(ui, t, "Gradient line", "pane.format.line.gradient", is(PaintKind::Gradient), enabled) && !is(PaintKind::Gradient) {
            send = Some(("format.gradient", json!({"target": "line", "on": true})));
        }
        if let Some((command, params)) = send {
            self.pane_send(command, params);
        }
        let drawn = enabled && !is(PaintKind::None);
        self.line_details(ui, t, drawn);
        self.weight_row(ui, t, drawn);
        self.line_styles(ui, t, drawn);
        let (dash, begin, end) = (self.pattern_label(), self.arrow_label(true), self.arrow_label(false));
        self.list_button(ui, t, "Dash type", "pane.format.line.dash", DASH_ITEMS, drawn, dash);
        self.list_button(ui, t, "Begin arrow", "pane.format.line.begin_arrow", BEGIN_ARROW_ITEMS, drawn, begin);
        self.list_button(ui, t, "End arrow", "pane.format.line.end_arrow", END_ARROW_ITEMS, drawn, end);
        self.arrow_sizes(ui, t, drawn);
        self.rounding_row(ui, t, enabled);
    }

    fn effects_section(&mut self, ui: &mut Ui, t: &Tokens, selected: bool) {
        let heading = ui.label(egui::RichText::new("Shadow").font(theme::font(size::TAB)).color(t.text));
        self.note("pane.format.effects.0", heading.rect, true);
        ui.add_space(2.0);
        self.shadow_section(ui, t, selected);
        ui.add_space(8.0);
        for (n, effect) in EFFECTS.iter().enumerate() {
            self.quiet_pane_row(ui, t, effect, &format!("pane.format.effects.{}", n + 1));
            ui.add_space(2.0);
        }
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new("Reflection, glow, soft edges, bevel and 3-D are not set by any command yet.")
                    .font(theme::font(size::LABEL))
                    .color(t.text_dim),
            )
            .wrap(),
        );
    }

    /// Sends `format.shape` with `params` for the selection: one change, one undo step, and the picture follows.
    fn pane_format(&mut self, params: serde_json::Value) {
        self.pane_send("format.shape", params);
    }

    /// A radio button of the pane. → whether it was pressed.
    fn pane_radio(&mut self, ui: &mut Ui, t: &Tokens, label: &str, name: &str, selected: bool, enabled: bool) -> bool {
        let r = ui.add_enabled(
            enabled,
            egui::RadioButton::new(selected, egui::RichText::new(label).font(theme::font(size::LABEL)).color(label_colour(t, enabled))),
        );
        self.note(name.to_owned(), r.rect, enabled);
        r.clicked()
    }

    /// The weight in points: a field that takes units and arithmetic, and two buttons that step a quarter point.
    fn weight_row(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        // A typed value is one command for every shape, and a command takes at most MAX_SHAPES: with more the box is quiet, as Size & Position's are.
        let count = self.selection().len();
        let (enabled, why) = if count > MAX_SHAPES {
            (false, format!("{count} shapes are selected: one command takes at most {MAX_SHAPES}, so a weight is not typed. Select fewer shapes."))
        } else {
            (enabled, "The line has no weight to set".to_owned())
        };
        let weight = self.weight_mix();
        let shown = match weight {
            Mix::One(thousandths) => Some(format!("{} pt", number_text(thousandths as f64 / 1000.0))),
            _ => None,
        };
        let shapes = self.format_target();
        let target = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
        self.pane_field(ui, t, Field { label: "Width", name: "pane.format.line.weight", shown, enabled, why_not: Some(&why), target });
        if let Mix::One(thousandths) = weight {
            let points = thousandths as f64 / 1000.0;
            ui.horizontal(|ui| {
                ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
                for (label, name, delta) in [("-", "pane.format.line.weight.down", -WEIGHT_STEP), ("+", "pane.format.line.weight.up", WEIGHT_STEP)] {
                    let next = points + delta;
                    let ok = enabled && next > 0.0;
                    let r =
                        ui.add_enabled(ok, egui::Button::new(egui::RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(28.0, 20.0)));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ok, if delta < 0.0 { "Thinner line" } else { "Thicker line" }));
                    self.note(name, r.rect, ok);
                    if r.clicked() && ok {
                        self.pane_format(json!({"weight": next}));
                    }
                }
            });
        }
    }

    /// The rounding of the corners, in the page's unit.
    fn rounding_row(&mut self, ui: &mut Ui, t: &Tokens, enabled: bool) {
        let count = self.selection().len();
        let (enabled, why) = if count > MAX_SHAPES {
            (false, format!("{count} shapes are selected: one command takes at most {MAX_SHAPES}, so a rounding is not typed. Select fewer shapes."))
        } else {
            (enabled, "Nothing is selected".to_owned())
        };
        let Some(page) = self.current_page() else { return };
        let Some(settings) = self.session().document().and_then(|d| crate::canvas::page_drawing::page_settings(d, page)) else { return };
        let unit = settings.ruler_unit;
        let rounding = self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| (l.rounding * 1.0e6).round() as i64));
        let shown = match rounding {
            Mix::One(micro) => Some(length_text(micro as f64 / 1.0e6, unit)),
            _ => None,
        };
        let shapes = self.format_target();
        let target = shapes.as_ref().map(|(page, ids)| Target { page: *page, ids });
        self.pane_field(ui, t, Field { label: "Rounding", name: "pane.format.line.rounding", shown, enabled, why_not: Some(&why), target });
    }

    /// The shapes a weight or a rounding box shows, and their page: what a value typed in it is for.
    pub(super) fn format_target(&mut self) -> Option<(u32, Vec<u32>)> {
        self.selection_here()
    }

    /// Sends a value typed in a box of the pane to the shapes it showed: one command with their ids. The weight and the rounding are `shape.format`'s, the stops of a
    /// gradient `shape.gradient`'s (through the stops the shape has), the shadow's `shape.shadow`'s.
    pub(super) fn send_format_typed(&mut self, typed: Typed) {
        let Typed { name, text, page, ids } = typed;
        let Some(settings) = self.session().document().and_then(|d| crate::canvas::page_drawing::page_settings(d, page)) else { return };
        let unit = settings.ruler_unit;
        let percent = |what: &str| -> Result<f64, String> {
            let n: f64 = text.trim().trim_end_matches('%').trim().parse().map_err(|_| format!("{what} is a number from 0 to 100 percent."))?;
            if (0.0..=100.0).contains(&n) { Ok(n) } else { Err(format!("{what} is from 0 to 100 percent.")) }
        };
        let params: Result<(&'static str, serde_json::Value), String> = (|| {
            if ids.len() > MAX_SHAPES {
                return Err(format!("{} shapes are more than one command takes ({MAX_SHAPES}): select fewer shapes.", ids.len()));
            }
            let at = |mut p: serde_json::Value| {
                if let Some(map) = p.as_object_mut() {
                    map.insert("ids".to_owned(), json!(ids));
                    map.insert("page".to_owned(), json!(page));
                }
                p
            };
            let points = |what: &str, most: f64| -> Result<f64, String> {
                let inches = parse_length(&text, Unit::Point)?;
                let p = (Unit::Point.from_internal(inches) * 1000.0).round() / 1000.0;
                if !(0.0..=most).contains(&p) { Err(format!("{what} is from 0 to {} pt.", number_text(most))) } else { Ok(p) }
            };
            let target = |n: &str| if n.contains(".line.") { "line" } else { "fill" };
            match name.as_str() {
                "pane.format.line.weight" => {
                    let inches = parse_length(&text, Unit::Point)?;
                    let points = (Unit::Point.from_internal(inches) * 1000.0).round() / 1000.0;
                    if points <= 0.0 {
                        return Err("A weight is more than nothing.".to_owned());
                    }
                    if points > MAX_WEIGHT_POINTS {
                        return Err(format!("A weight is at most {} pt.", number_text(MAX_WEIGHT_POINTS)));
                    }
                    Ok(("shape.format", at(json!({"weight": points}))))
                }
                "pane.format.line.rounding" => {
                    let drawing = parse_length(&text, unit)?;
                    if drawing < 0.0 {
                        return Err("A rounding is not less than nothing.".to_owned());
                    }
                    let paper = (drawing * settings.drawing_scale * 1.0e6).round() / 1.0e6;
                    if paper > MAX_ROUNDING_PAPER_INCHES {
                        return Err(format!("A rounding is at most {}.", length_text(MAX_ROUNDING_PAPER_INCHES / settings.drawing_scale, unit)));
                    }
                    Ok(("shape.format", at(json!({"rounding": paper}))))
                }
                "pane.format.fill.transparency" => Ok(("shape.format", at(json!({"fill_transparency": percent("Transparency")?})))),
                "pane.format.line.transparency" => Ok(("shape.format", at(json!({"line_transparency": percent("Transparency")?})))),
                n if n.ends_with(".gradient.angle") => {
                    let degrees = crate::measure::parse_angle(&text)?;
                    Ok(("shape.gradient", at(json!({"target": target(n), "angle": degrees}))))
                }
                n if n.ends_with(".gradient.stop.position") || n.ends_with(".gradient.stop.transparency") => {
                    // The stops are the shape's, so the window command works them out; the ids of the box are the shapes it showed.
                    let key = if n.ends_with("position") { "stop_position" } else { "stop_transparency" };
                    let value = percent(if n.ends_with("position") { "A position" } else { "Transparency" })?;
                    let value = if key == "stop_position" { value / 100.0 } else { value };
                    self.send_stop_typed(n, key, value, page, &ids).map(|()| ("", json!(null)))
                }
                "pane.format.shadow.transparency" => Ok(("shape.shadow", at(json!({"transparency": percent("Transparency")?})))),
                "pane.format.shadow.size" => {
                    let n: f64 = text.trim().trim_end_matches('%').trim().parse().map_err(|_| "A size is a number of percent.".to_owned())?;
                    if !(1.0..=1000.0).contains(&n) {
                        return Err("A size is from 1 to 1000 percent.".to_owned());
                    }
                    Ok(("shape.shadow", at(json!({"size": n}))))
                }
                "pane.format.shadow.blur" => Ok(("shape.shadow", at(json!({"blur": points("A blur", 1000.0)?})))),
                "pane.format.shadow.distance" => Ok(("shape.shadow", at(json!({"distance": points("A distance", 1000.0)?})))),
                "pane.format.shadow.angle" => Ok(("shape.shadow", at(json!({"angle": crate::measure::parse_angle(&text)?})))),
                _ => Err("That field takes no value.".to_owned()),
            }
        })();
        let sent = match params {
            // A stop's number was sent already, by the window command.
            Ok(("", _)) => Ok(()),
            Ok((command, params)) => self.execute(command, &params).map(|_| ()).map_err(|e| e.to_string()),
            Err(words) => Err(words),
        };
        match sent {
            Ok(()) => self.field_accepted(),
            Err(words) => self.field_refused(&name, words, page, &ids),
        }
    }

    /// A position or a transparency typed for the stop selected: `format.gradient` with the stop's edit, for the shapes the box showed.
    fn send_stop_typed(&mut self, name: &str, key: &str, value: f64, page: u32, ids: &[u32]) -> Result<(), String> {
        let target = if name.contains(".line.") { "line" } else { "fill" };
        // The window command works on the shapes selected: the box's shapes are those unless the selection changed under it, which the field's own rule covers.
        if self.selection_here() != Some((page, ids.to_vec())) {
            return Err("The shapes the box showed are not selected any more.".to_owned());
        }
        self.execute("format.gradient", &json!({"target": target, key: value})).map(|_| ()).map_err(|e| e.to_string())
    }

    /// The dash type's name for the selection, or nothing when the shapes differ.
    fn pattern_label(&self) -> Option<&'static str> {
        match self.pattern_mix() {
            Mix::One(n) => DASH_ITEMS.iter().find(|i| i.sample == spec::Sample::Dash(u8::try_from(n).unwrap_or(0))).map(|i| i.label),
            _ => None,
        }
    }

    fn arrow_label(&self, begin: bool) -> Option<&'static str> {
        let (mix, items) = if begin { (self.begin_arrow_mix(), BEGIN_ARROW_ITEMS) } else { (self.end_arrow_mix(), END_ARROW_ITEMS) };
        match mix {
            Mix::One(n) => items.iter().find(|i| i.sample == spec::Sample::Arrow { number: u8::try_from(n).unwrap_or(0), begin }).map(|i| i.label),
            _ => None,
        }
    }

    /// A labelled box with a drop-down arrow that opens a list of the Home tab's rows (a dash or a line end): the row of what the selection has is marked, and
    /// a press sends the row's command.
    #[allow(clippy::too_many_arguments)]
    fn list_button(&mut self, ui: &mut Ui, t: &Tokens, label: &str, name: &str, items: &'static [Ctl], enabled: bool, current: Option<&'static str>) {
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
                current.unwrap_or(""),
                theme::font(size::LABEL),
                label_colour(t, enabled),
            );
            crate::icons::chevron(ui.painter(), pos2(rect.right() - 8.0, rect.center().y), 2.5, label_colour(t, enabled));
            self.note(name.to_owned(), rect, enabled);
            let cx = Cx { t: *t, tab: "pane" };
            if enabled {
                egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
                    ui.set_width(menu_width(ui, items, false));
                    egui::ScrollArea::vertical().max_height(300.0).auto_shrink([true, true]).show(ui, |ui| {
                        for item in items {
                            menu_row(self, ui, cx, item);
                        }
                    });
                });
            }
        });
    }
}
