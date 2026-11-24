//! The Snap & Glue dialog: the Visual Aids group's launcher (`SnapAndGlueDialog`) and Alt+F9 (
//! "Snap on or off": "a document-wide toggle in Snap and Glue decides whether shapes pull into alignment while being drawn or moved. Alt+F9 opens the
//! dialog").
//!
//! Two tabs, as the brief of the task lays them out from the feature rows ("Snap strength", "Shape extension lines" and "Isometric angle lines" cite the
//! dialog's Advanced tab):
//!
//! * **General**: the **Snap and Glue switches** (`snap`, `glue`, DiaCraft's own part), each at the head of the column of what it governs: what
//!   a shape snaps to (the flags of `SnapSettings`: "`SnapSettings` to the boxes under Snap to") and what a connector's end glues
//!   to (the flags of `GlueSettings`: "those under Glue to");
//! * **Advanced**: the **snap strength**: six rows, Rulers, Grid, Guides, Points, Geometry and Extensions, each a
//!   label, a slider and a number box, under "Weak" and "Strong" and "Pixels", and the **shape extension lines** (the thirteen flags of `SnapExtensions`).
//!
//! **Every box starts from `snap.settings`**, read when the dialog opens. The dialog works on a draft (as the Text dialog and Page Setup do): **OK sends one
//! `snap.set`** with the options and the strengths that were changed (no undo step: a View setting is "unsaved, not undo"), and nothing when none was; Cancel and Escape send nothing. A refusal (an option whose
//! word in the file is text) is said in the dialog, which stays open.
//!
//! **A strength is a number of screen pixels from 0 to 999**, as the number box takes it and the engine's `snap.set` checks it. The slider's travel is 0 to
//! [`SLIDER_MAX`] (the spec's one image shows about 30 steps), so a number past it puts the thumb at the strong end; the slider and the box change together.
//! **A box that does not say a whole number of pixels from 0 to 999** is marked, and OK is refused in words that name the row (the dialog stays open and sends nothing; the
//! spec does not say what the reference app's box does with 1000).
//!
//! **Left out, and why**: the **isometric angles** (`SnapAngles`, kept empty; the tab says so). The extension lines and Snap to "Shape extensions" are settings,
//! saved in the file's words, that nothing draws yet: they are drawn working, with a line that says so. The dynamic grid is the Visual Aids group's box, not the
//! dialog's.
//!
//! **The captions are ours**, from the words of the specification's tables (no source read gives the dialog's own); the boxes' accessible names say which list a box is
//! in ("Snap to guides", "Glue to guides").

use diacraft_engine::snap::{OPTIONS, STRENGTH_MAX, STRENGTHS, SnapSettings, StrengthDef, strength_pixels};
use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The tabs of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapTab {
    General,
    Advanced,
}

impl SnapTab {
    pub const ALL: [SnapTab; 2] = [SnapTab::General, SnapTab::Advanced];

    pub const fn name(self) -> &'static str {
        match self {
            SnapTab::General => "General",
            SnapTab::Advanced => "Advanced",
        }
    }

    pub fn from_name(name: &str) -> Option<SnapTab> {
        SnapTab::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The most the content of the tab is tall, in points: General holds the two columns of boxes, Advanced the thirteen extension lines and the six strength rows.
    const fn content_max_h(self) -> f32 {
        match self {
            SnapTab::General => 340.0,
            SnapTab::Advanced => 480.0,
        }
    }
}

/// A box of the dialog: the option it stands for (its name in `snap.settings` and `snap.set`) and its caption.
pub type Row = (&'static str, &'static str);

/// What a shape snaps to: the flags of `SnapSettings`, in the order of the table.
pub const SNAP_TO: &[Row] = &[
    ("ruler", "Ruler subdivisions"),
    ("grid", "Grid"),
    ("guides", "Guides"),
    ("snap_handles", "Selection handles"),
    ("snap_vertices", "Vertices"),
    ("snap_connection_points", "Connection points"),
    ("snap_geometry", "Shape outlines"),
    ("alignment_box", "Alignment box"),
    ("snap_extensions", "Shape extensions"),
    ("intersections", "Intersections"),
];

/// What a connector's end glues to: the flags of `GlueSettings`, in the order of the table.
pub const GLUE_TO: &[Row] = &[
    ("glue_guides", "Guides"),
    ("glue_handles", "Shape handles"),
    ("glue_vertices", "Shape vertices"),
    ("glue_connection_points", "Connection points"),
    ("glue_geometry", "Shape geometry"),
];

/// The shape extension lines: the flags of `SnapExtensions`, in the order of the table.
pub const EXTENSIONS: &[Row] = &[
    ("ext_alignment_box", "Alignment box extension"),
    ("ext_centre_axes", "Center axes"),
    ("ext_tangents", "Tangents inside curves"),
    ("ext_segment_ends", "Segment end points"),
    ("ext_segment_midpoints", "Segment midpoints"),
    ("ext_linear", "Linear extension"),
    ("ext_curved", "Curved extension"),
    ("ext_perpendicular_end", "Perpendicular at an end point"),
    ("ext_perpendicular_mid", "Perpendicular at a midpoint"),
    ("ext_horizontal_end", "Horizontal line at an end point"),
    ("ext_vertical_end", "Vertical line at an end point"),
    ("ext_ellipse_centres", "Ellipse center points"),
    ("ext_isometric", "Isometric angle lines"),
];

/// The two switches, each at the head of its column.
pub const SNAP_SWITCH: Row = ("snap", "Snap");
pub const GLUE_SWITCH: Row = ("glue", "Glue");

/// Every option the dialog has a box for: all of `snap.settings` but the dynamic grid, which is the Visual Aids group's.
pub fn boxed_options() -> Vec<&'static str> {
    let mut all = vec![SNAP_SWITCH.0, GLUE_SWITCH.0];
    all.extend(SNAP_TO.iter().chain(GLUE_TO).chain(EXTENSIONS).map(|(name, _)| *name));
    all
}

/// The settings `snap.settings` answered with: each option under its name (one the answer lacks keeps its default).
fn settings_of(answer: &Value) -> SnapSettings {
    let mut settings = SnapSettings::default();
    for (index, option) in OPTIONS.iter().enumerate() {
        if let Some(on) = answer.get(option.name).and_then(Value::as_bool) {
            settings.put(index, on);
        }
    }
    for (index, def) in STRENGTHS.iter().enumerate() {
        if let Some(pixels) = answer.get(def.name).and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok()) {
            settings.strength.put(index, pixels);
        }
    }
    settings
}

/// The value of the option `name` in `settings`; off for a name that is none.
fn option(settings: &SnapSettings, name: &str) -> bool {
    SnapSettings::index_of(name).is_some_and(|i| settings.get(i))
}

/// The Snap & Glue dialog while it is open.
pub struct SnapDialog {
    pub(crate) tab: SnapTab,
    /// The settings as `snap.settings` gave them when the dialog opened.
    start: SnapSettings,
    /// What the boxes show now (the strengths are the last good number of each row's box).
    draft: SnapSettings,
    /// What each strength's number box says now, as typed: it is `draft`'s number until the person types, and may then be text that is no number of pixels.
    texts: [String; 6],
    error: Option<String>,
    /// Whether `error` is OK's refusal of a number box (it goes when the boxes are right again), and not an answer of the engine.
    refused_numbers: bool,
}

impl SnapDialog {
    /// A dialog on `tab` that starts from `start`.
    fn new(tab: SnapTab, start: SnapSettings) -> SnapDialog {
        let texts = std::array::from_fn(|i| start.strength.get(i).to_string());
        SnapDialog { tab, start, draft: start, texts, error: None, refused_numbers: false }
    }

    /// The `snap.set` parameters of what was changed: every option and every strength whose box differs from what the dialog opened with.
    fn params(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for (index, option) in OPTIONS.iter().enumerate() {
            if self.draft.get(index) != self.start.get(index) {
                out.insert(option.name.to_owned(), Value::Bool(self.draft.get(index)));
            }
        }
        for (index, def) in STRENGTHS.iter().enumerate() {
            if self.draft.strength.get(index) != self.start.strength.get(index) {
                out.insert(def.name.to_owned(), json!(self.draft.strength.get(index)));
            }
        }
        out
    }

    /// Sets the number of the strength row `index`, from the slider or the typed text: the draft, and the box's text.
    fn set_strength(&mut self, index: usize, pixels: u16) {
        if self.draft.strength.put(index, pixels)
            && let Some(text) = self.texts.get_mut(index)
        {
            *text = pixels.to_string();
        }
    }

    /// The box of row `index` was typed in: its number is the draft's when the text says one (a whole number of pixels from 0 to 999).
    fn typed(&mut self, index: usize) {
        if let Some(pixels) = self.texts.get(index).and_then(|t| strength_pixels(t)) {
            self.draft.strength.put(index, pixels);
        }
        if self.refused_numbers && self.refusal().is_none() {
            self.error = None;
            self.refused_numbers = false;
        }
    }

    /// Why OK is refused: the first row whose box does not say a whole number of pixels from 0 to 999.
    fn refusal(&self) -> Option<String> {
        STRENGTHS
            .iter()
            .zip(&self.texts)
            .find(|(_, text)| strength_pixels(text).is_none())
            .map(|(def, _)| format!("{} strength: type a whole number of screen pixels from 0 to {STRENGTH_MAX}.", def.label))
    }

    fn toggle(&mut self, name: &str) {
        if let Some(index) = SnapSettings::index_of(name) {
            let on = self.draft.get(index);
            self.draft.put(index, !on);
        }
    }
}

impl DiacraftApp {
    /// Whether the Snap & Glue dialog is open, and on which tab.
    pub fn snap_dialog_tab(&self) -> Option<&'static str> {
        self.ui.snap_dialog.as_ref().map(|d| d.tab.name())
    }

    /// What the dialog's box for the option `name` shows now (`None`: the dialog is closed, or has no such box). For tests and agents.
    pub fn snap_dialog_shows(&self, name: &str) -> Option<bool> {
        let dialog = self.ui.snap_dialog.as_ref()?;
        boxed_options().contains(&name).then(|| option(&dialog.draft, name))
    }

    /// The number the dialog's slider for the strength `name` (`strength_grid`) shows now, as `snap.settings` names it (`None`: the dialog is closed, or has no such row). For
    /// tests and agents.
    pub fn snap_dialog_strength(&self, name: &str) -> Option<u16> {
        let dialog = self.ui.snap_dialog.as_ref()?;
        Some(dialog.draft.strength.get(STRENGTHS.iter().position(|d| d.name == name)?))
    }

    /// Why the dialog's OK was refused, while it stays open (`None`: it was not, or the dialog is closed).
    pub fn snap_dialog_error(&self) -> Option<&str> {
        self.ui.snap_dialog.as_ref().and_then(|d| d.error.as_deref())
    }

    /// Opens the dialog on `tab`, with the document's settings; one that is open changes tab (and keeps what was chosen).
    pub(crate) fn open_snap_dialog(&mut self, tab: SnapTab) -> diacraft_engine::Result<()> {
        if let Some(dialog) = self.ui.snap_dialog.as_mut() {
            dialog.tab = tab;
            return Ok(());
        }
        let answer = self.execute("snap.settings", &json!({}))?;
        self.ui.snap_dialog = Some(SnapDialog::new(tab, settings_of(&answer)));
        Ok(())
    }

    pub(crate) fn close_snap_dialog(&mut self) {
        self.ui.snap_dialog = None;
    }

    /// OK: one `snap.set` with what was changed, or nothing. On a refusal the dialog stays open and says why.
    fn apply_snap_dialog(&mut self) {
        let Some(dialog) = self.ui.snap_dialog.as_mut() else { return };
        // A number box that says no number of pixels: said in words, and nothing sent (the box keeps what was typed).
        if let Some(why) = dialog.refusal() {
            dialog.error = Some(why);
            dialog.refused_numbers = true;
            return;
        }
        dialog.error = None;
        dialog.refused_numbers = false;
        let params = dialog.params();
        if params.is_empty() {
            self.close_snap_dialog();
            return;
        }
        match self.execute("snap.set", &Value::Object(params)) {
            Ok(_) => self.close_snap_dialog(),
            Err(e) => {
                if let Some(d) = self.ui.snap_dialog.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    /// Whether the document has the Snap & Glue option the parameters `args` of a check box name (`{"dynamic_grid":true}`: the key is the option).
    pub(crate) fn snap_box_checked(&self, args: &str) -> bool {
        let Some(name) = box_option(args) else { return false };
        self.snap_settings_now().is_some_and(|s| option(&s, &name))
    }

    /// The `snap.set` a press on such a box sends: its option the other way from how the document has it. `None` for parameters that name no option.
    pub(crate) fn snap_box_toggle(&self, args: &str) -> Option<Value> {
        let name = box_option(args)?;
        SnapSettings::index_of(&name)?;
        let on = self.snap_settings_now().is_some_and(|s| option(&s, &name));
        let mut params = Map::new();
        params.insert(name, Value::Bool(!on));
        Some(Value::Object(params))
    }

    pub(crate) fn snap_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 520.0);
        let Some(mut dialog) = self.ui.snap_dialog.take() else { return };
        // The content is as tall as its tab needs, to a size that depends on the tab and the window and on nothing the controls find (a dialog that changes its size as its
        // parts find theirs takes frames to settle in the middle of the window). In a short window the tab scrolls, with the scroll bar drawn.
        let content_h = (screen.height() - 190.0).clamp(140.0, dialog.tab.content_max_h());
        let (mut ok, mut cancel) = (false, false);
        // Enter is OK while no control has the keyboard; a control that has it (reached by Tab) takes Enter as a press of its own, as egui gives it, so
        // Enter on a box ticks it and on Cancel cancels. Taken before the controls are drawn, so that nothing else sees it.
        if !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            ok = true;
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("snap_glue_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Snap & Glue").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for tab in SnapTab::ALL {
                    let r =
                        ui.add(egui::Button::selectable(dialog.tab == tab, RichText::new(tab.name()).font(theme::font(size::LABEL)).color(t.text)));
                    seen.push((format!("snapglue.tab.{}", tab.name()), r.rect, true));
                    if r.clicked() {
                        dialog.tab = tab;
                    }
                }
            });
            ui.add_space(6.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let content = egui::ScrollArea::vertical()
                .id_salt("snap_glue_content")
                .min_scrolled_height(content_h)
                .max_height(content_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // As wide as the part that shows (the scroll bar has its own room beside it).
                    let inner = ui.available_width();
                    ui.set_width(inner);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    match dialog.tab {
                        SnapTab::General => general_tab(ui, &t, &mut dialog, &mut seen),
                        SnapTab::Advanced => advanced_tab(ui, &t, &mut dialog, &mut seen),
                    }
                });
            seen.push(("snapglue.content".to_owned(), content.inner_rect, true));
            ui.add_space(6.0);
            if let Some(error) = &dialog.error {
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("snapglue.error".to_owned(), said.rect, true));
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "snapglue.ok", true), ("Cancel", "snapglue.cancel", false)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if primary { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            cancel = true;
        }
        self.ui.snap_dialog = Some(dialog);
        if cancel {
            self.close_snap_dialog();
        } else if ok {
            self.apply_snap_dialog();
        }
    }
}

/// The option the parameters of a check box name: the one key of its JSON object.
fn box_option(args: &str) -> Option<String> {
    let value: Value = serde_json::from_str(args).ok()?;
    let object = value.as_object()?;
    match object.len() {
        1 => object.keys().next().cloned(),
        _ => None,
    }
}

fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
}

/// A line of quieter words that wraps: what a part of the dialog leaves out.
fn note(ui: &mut Ui, t: &Tokens, words: &str) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim)).wrap())
}

/// The height of a box's row, in points.
const ROW_H: f32 = 22.0;

/// A check box `width` wide for the option `name` of the draft, drawn as the Text dialog's boxes are: a press turns it the other way. `accessible` is the
/// name assistive technology hears, which says which list it is in.
fn check_box(
    ui: &mut Ui,
    t: &Tokens,
    d: &mut SnapDialog,
    (name, caption): Row,
    accessible: &str,
    width: f32,
    seen: &mut Vec<(String, egui::Rect, bool)>,
) {
    let on = option(&d.draft, name);
    let (rect, resp) = ui.allocate_exact_size(vec2(width.max(40.0), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, accessible));
    let painter = ui.painter_at(rect);
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::same(3), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    let b = egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(14.0, 14.0));
    painter.rect(b, CornerRadius::same(2), if on { t.accent } else { t.input_fill }, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    if on {
        let (a, m, z) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
        painter.line_segment([a, m], Stroke::new(1.6, t.accent_text));
        painter.line_segment([m, z], Stroke::new(1.6, t.accent_text));
    }
    painter.text(egui::pos2(rect.left() + 26.0, rect.center().y), egui::Align2::LEFT_CENTER, caption, theme::font(size::LABEL), t.text);
    seen.push((format!("snapglue.option.{name}"), rect, true));
    if resp.clicked() {
        d.toggle(name);
    }
}

/// The least width of a column of boxes, in points: the longest caption ("Horizontal line at an end point") with its box.
const COLUMN_MIN: f32 = 200.0;

/// Lays out the two parts `part` draws (`0` and `1`, each given its width) side by side where there is room for two columns, else one above the other.
fn two_columns(ui: &mut Ui, mut part: impl FnMut(&mut Ui, usize, f32)) {
    let width = ui.available_width();
    let gap = 16.0;
    if width >= 2.0 * COLUMN_MIN + gap {
        let column = (width - gap) / 2.0;
        ui.horizontal_top(|ui| {
            // The gap is the whole space between the columns.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.set_width(column);
                part(ui, 0, column);
            });
            ui.add_space(gap);
            ui.vertical(|ui| {
                ui.set_width(column);
                part(ui, 1, column);
            });
        });
    } else {
        part(ui, 0, width);
        ui.add_space(8.0);
        part(ui, 1, width);
    }
}

/// The switches and the targets: each switch heads the column of what it governs.
fn general_tab(ui: &mut Ui, t: &Tokens, d: &mut SnapDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    two_columns(ui, |ui, which, w| {
        let (switch, list, rows) = if which == 0 { (SNAP_SWITCH, "Snap to", SNAP_TO) } else { (GLUE_SWITCH, "Glue to", GLUE_TO) };
        check_box(ui, t, d, switch, switch.1, w, seen);
        ui.add_space(6.0);
        heading(ui, t, list);
        for row in rows {
            check_box(ui, t, d, *row, &format!("{list} {}", lower(row.1)), w, seen);
        }
        if which == 1 {
            // What is saved and not acted on yet (the review's): a connector's end pulled to a handle, a vertex or an outline lands there free, as
            // glue to them has no cell form yet, and nothing draws shape extensions.
            ui.add_space(8.0);
            let words = note(ui, t, GENERAL_NOTE);
            seen.push(("snapglue.note".to_owned(), words.rect, true));
        }
    });
}

/// The line under Glue to: the boxes that are saved with the drawing and do not act yet.
pub const GENERAL_NOTE: &str = "Saved, not acted on yet: a connector's end pulled to a shape handle, vertex or geometry is left there unglued, and Snap to \
                                shape extensions does nothing.";

/// The extension lines, then the snap strength sliders: the order of the specification's Advanced tab (shape extension options, then snap strength).
fn advanced_tab(ui: &mut Ui, t: &Tokens, d: &mut SnapDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Shape extension lines");
    let (first, second) = EXTENSIONS.split_at(EXTENSIONS.len().div_ceil(2));
    two_columns(ui, |ui, which, w| {
        for row in if which == 0 { first } else { second } {
            check_box(ui, t, d, *row, row.1, w, seen);
        }
    });
    ui.add_space(6.0);
    note(ui, t, "Saved with the drawing. DiaCraft draws no extension lines yet, and the angles of the isometric lines have no setting.");
    ui.add_space(10.0);
    heading(ui, t, "Snap strength");
    strength_header(ui, t);
    for (index, def) in STRENGTHS.iter().enumerate() {
        strength_row(ui, t, d, index, def, seen);
    }
    ui.add_space(2.0);
    let words = note(ui, t, STRENGTH_NOTE);
    seen.push(("snapglue.strength.note".to_owned(), words.rect, true));
}

/// The line under the strength rows: what the numbers are, in our words.
pub const STRENGTH_NOTE: &str = "Each number is how far, in screen pixels, that kind of target pulls a shape. A low number lets a shape stick to whatever is near. A high \
                                 one keeps it to that kind even when others are near. Extensions also sets how far the dynamic grid's alignment lines pull.";

/// The most the slider reaches: the number box goes to [`STRENGTH_MAX`], and a number past this puts the thumb at the strong end.
pub const SLIDER_MAX: u16 = 30;

/// The width of a row's label, and of its number box, in points.
const LABEL_W: f32 = 76.0;
const BOX_W: f32 = 46.0;

/// The width of a row's slider in a content `width` points wide: what is left of the row after its label and its number box and the two gaps between.
pub(crate) fn slider_width(width: f32, gap: f32) -> f32 {
    (width - LABEL_W - BOX_W - 2.0 * gap).max(60.0)
}

/// The column headings above the rows: "Weak" at the left end of the slider, "Strong" at its right end, and "Pixels" over the number boxes.
fn strength_header(ui: &mut Ui, t: &Tokens) {
    let width = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    let slider = slider_width(width, gap);
    let (left, box_left) = (rect.left() + LABEL_W + gap, rect.left() + LABEL_W + gap + slider + gap);
    let cell = |x0: f32, x1: f32| egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom()));
    let heading = |ui: &mut Ui, at: egui::Rect, layout: egui::Layout, words: &str| {
        ui.scope_builder(egui::UiBuilder::new().max_rect(at).layout(layout), |ui| {
            ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
        });
    };
    heading(ui, cell(left, left + slider / 2.0), egui::Layout::left_to_right(egui::Align::Center), "Weak");
    heading(ui, cell(left + slider / 2.0, left + slider), egui::Layout::right_to_left(egui::Align::Center), "Strong");
    heading(ui, cell(box_left, box_left + BOX_W), egui::Layout::centered_and_justified(egui::Direction::TopDown), "Pixels");
}

/// One row of the snap strength: the label, the slider (0 to [`SLIDER_MAX`]) and the number box (0 to 999), changing together. The slider is named "Grid snap strength" for
/// assistive technology, and the box says what it is in pixels.
fn strength_row(ui: &mut Ui, t: &Tokens, d: &mut SnapDialog, index: usize, def: &StrengthDef, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let key = def.name.trim_start_matches("strength_");
    ui.horizontal(|ui| {
        let width = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        ui.add_sized([LABEL_W, ROW_H], egui::Label::new(RichText::new(format!("{}:", def.label)).font(theme::font(size::LABEL)).color(t.text)));
        let slider_w = slider_width(width, gap);
        ui.spacing_mut().slider_width = slider_w;
        let mut pixels = d.draft.strength.get(index);
        let accessible = format!("{} snap strength", def.label);
        let slider = ui.add_sized(
            [slider_w, ROW_H],
            egui::Slider::new(&mut pixels, 0..=SLIDER_MAX).show_value(false).clamping(egui::SliderClamping::Never).trailing_fill(true),
        );
        slider.widget_info(|| egui::WidgetInfo::slider(true, f64::from(pixels), accessible.clone()));
        let slider = slider.accessible_name(accessible);
        seen.push((format!("snapglue.strength.{key}.slider"), slider.rect, true));
        if slider.changed() {
            d.set_strength(index, pixels);
        }
        let valid = d.texts.get(index).is_some_and(|text| strength_pixels(text).is_some());
        let text = d.texts.get_mut(index);
        if let Some(text) = text {
            let edit = ui.add_sized(
                [BOX_W, ROW_H],
                egui::TextEdit::singleline(text).char_limit(4).align(egui::Align2::CENTER_CENTER).font(theme::font(size::LABEL)),
            );
            let edit = edit.accessible_name(format!("{} snap strength in pixels", def.label));
            if !valid {
                ui.painter().rect_stroke(edit.rect, CornerRadius::same(2), Stroke::new(1.5, t.notice_text), StrokeKind::Inside);
            }
            seen.push((format!("snapglue.strength.{key}.box"), edit.rect, true));
            if edit.changed() {
                d.typed(index);
            }
        }
    });
}

/// A caption as it reads after "Snap to" or "Glue to": its first letter small.
fn lower(caption: &str) -> String {
    let mut chars = caption.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_has_a_box_for_every_option_but_the_dynamic_grid_and_none_twice() {
        let mut boxed = boxed_options();
        let n = boxed.len();
        boxed.sort_unstable();
        boxed.dedup();
        assert_eq!(boxed.len(), n, "an option has two boxes");
        let mut all: Vec<&str> = OPTIONS.iter().map(|o| o.name).filter(|n| *n != "dynamic_grid").collect();
        all.sort_unstable();
        assert_eq!(boxed, all);
    }

    #[test]
    fn ok_sends_only_what_was_changed() {
        let mut d = SnapDialog::new(SnapTab::General, SnapSettings::default());
        assert!(d.params().is_empty());
        d.toggle("grid");
        d.toggle("glue");
        d.toggle("ext_isometric");
        assert_eq!(Value::Object(d.params()), json!({"grid": false, "glue": false, "ext_isometric": true}));
        d.toggle("grid");
        assert_eq!(Value::Object(d.params()), json!({"glue": false, "ext_isometric": true}), "a box turned back is no change");
    }

    #[test]
    fn the_settings_are_read_from_the_answer_of_snap_settings() {
        let read = settings_of(&json!({"snap": false, "grid": false, "glue_geometry": true, "words": {}}));
        assert!(!read.snap && !read.grid && read.glue_geometry && read.glue && read.ruler);
    }

    #[test]
    fn the_strengths_are_read_from_the_answer_and_sent_when_they_differ() {
        let read = settings_of(&json!({"strength_ruler": 0, "strength_grid": 999, "strength_guides": "ten", "strength_points": 1000, "words": {}}));
        assert_eq!(
            (read.strength.ruler, read.strength.grid, read.strength.guides, read.strength.points),
            (0, 999, 10, 10),
            "text and 1000 are not numbers of pixels"
        );
        let mut d = SnapDialog::new(SnapTab::Advanced, SnapSettings::default());
        assert_eq!(d.texts, ["4", "5", "10", "10", "8", "13"]);
        d.set_strength(1, 9);
        d.set_strength(5, 40);
        d.toggle("grid");
        assert_eq!(Value::Object(d.params()), json!({"grid": false, "strength_grid": 9, "strength_extensions": 40}));
        assert_eq!(d.texts[1], "9", "the box follows the slider");
        d.set_strength(1, 5);
        assert_eq!(Value::Object(d.params()), json!({"grid": false, "strength_extensions": 40}), "a number put back is no change");
        assert!(!d.draft.strength.put(2, 1000), "past 999 is none");
        d.set_strength(2, 1000);
        assert_eq!(d.draft.strength.get(2), 10, "and the draft keeps its number");
    }

    #[test]
    fn a_typed_number_is_the_drafts_when_it_is_a_whole_number_of_pixels_and_ok_is_refused_for_one_that_is_not() {
        let mut d = SnapDialog::new(SnapTab::Advanced, SnapSettings::default());
        assert_eq!(d.refusal(), None);
        d.texts[3] = "37".to_owned();
        d.typed(3);
        assert_eq!(d.draft.strength.points, 37);
        for bad in ["", "1000", "-1", "3.5", "ten", " 7"] {
            d.texts[1] = bad.to_owned();
            d.typed(1);
            assert_eq!(d.draft.strength.grid, 5, "{bad:?} is no number: the draft is as it was");
            assert_eq!(d.refusal().as_deref(), Some("Grid strength: type a whole number of screen pixels from 0 to 999."), "{bad:?}");
        }
        d.texts[1] = "0".to_owned();
        d.typed(1);
        assert_eq!((d.refusal(), d.draft.strength.grid), (None, 0));
        assert_eq!(Value::Object(d.params()), json!({"strength_grid": 0, "strength_points": 37}));
    }

    #[test]
    fn a_box_names_its_option_by_the_one_key_of_its_parameters() {
        assert_eq!(box_option(r#"{"dynamic_grid":true}"#).as_deref(), Some("dynamic_grid"));
        assert_eq!(box_option(r#"{"a":true,"b":true}"#), None);
        assert_eq!(box_option("not json"), None);
        assert_eq!(lower("Ruler subdivisions"), "ruler subdivisions");
    }
}
