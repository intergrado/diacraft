//! Page Setup from the window: what the Design tab's Page Setup group shows of the page on view, and the Page Setup dialog.
//!
//! The group's controls (Orientation, Size, Auto Size) send the engine's page commands (`page.orientation`, `page.size`, `page.auto_size`;
//! `docs/editing-commands.md` "Page setup") and are marked from `page.setup`'s state, read once for each revision of the drawing
//! ([`DiacraftApp::refresh_page_view`]). **The dialog** has the tabs of (Print Setup, Page Size, Drawing Scale, Page
//! Properties, Layout and Routing, Alt Text). It works on a **draft**: nothing reaches the drawing until OK, which sends what the person changed as
//! the page commands (one command, or one `command.batch` that is one undo step), and Cancel or Escape sends nothing. **The Layout and Routing tab**
//! (`page_setup/routing.rs`, its Spacing dialog `routing_spacing.rs` and its preview `routing_preview.rs`) reads `page.routing` and adds a
//! `page.set_routing` and a `connector.line_jumps` to what OK sends, always in a batch. **The Alt Text tab** (`page_setup/alt_text.rs`) holds the page's Title and Description and
//! adds a `page.alt_text` with the text that was changed.
//!
//! **What the Print Setup tab cannot change yet** (the engine has no command for any of it: left print margins and centring out and the
//! printing task reads the cells without writing them): print zoom (`ScaleX`, `OnPage`, `PagesX`, `PagesY`), the four margins, centring and the
//! grid. They are drawn quiet, with the values the page's cells hold.
//!
//! Choices no specification settles (ours): the dialog applies on OK and not as it goes; a custom size is typed in inches; turning the orientation on
//! the Page Size tab swaps the two typed sizes; Insert in the page tab's menu makes the page first and opens the dialog on it (the reference app opens the dialog
//! first, which needs a page that does not exist yet).

use diacraft_engine::draw::paper::{Orientation, PAPERS, SCALES, SIZES};
use diacraft_engine::draw::{PageSetup, length_unit, read_setup};
use diacraft_engine::{EngineError, Result, shown};
use diacraft_render::print::PrintSetup;
use diacraft_units::Unit;
use egui::{Context, Id, Key, Modifiers, RichText, Ui};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

mod alt_text;
mod routing;
mod routing_preview;
mod routing_spacing;

/// The tabs of the dialog, in the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogTab {
    PrintSetup,
    PageSize,
    DrawingScale,
    PageProperties,
    LayoutAndRouting,
    AltText,
}

impl DialogTab {
    pub const ALL: [DialogTab; 6] = [
        DialogTab::PrintSetup,
        DialogTab::PageSize,
        DialogTab::DrawingScale,
        DialogTab::PageProperties,
        DialogTab::LayoutAndRouting,
        DialogTab::AltText,
    ];

    /// The tab's label.
    pub const fn name(self) -> &'static str {
        match self {
            DialogTab::PrintSetup => "Print Setup",
            DialogTab::PageSize => "Page Size",
            DialogTab::DrawingScale => "Drawing Scale",
            DialogTab::PageProperties => "Page Properties",
            DialogTab::LayoutAndRouting => "Layout and Routing",
            DialogTab::AltText => "Alt Text",
        }
    }

    /// The tab labelled `name`, in any case.
    pub fn from_name(name: &str) -> Option<DialogTab> {
        DialogTab::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// The units a custom drawing scale is given in (the engine's `length_unit`), as the dialog's lists spell them.
const UNITS: [&str; 8] = ["in", "ft", "yd", "mi", "mm", "cm", "m", "km"];

/// The most characters a typed number takes in the dialog.
const FIELD_CHARS: usize = 64;

/// The most characters a page's name takes in the dialog: a page name has at most 31 characters (the engine refuses a longer one).
const NAME_CHARS: usize = diacraft_engine::doc::MAX_PAGE_NAME_CHARS;

/// The most background pages the Background box lists.
const MAX_BACKGROUNDS_LISTED: usize = 500;

/// What the dialog has typed and chosen, apart from the drawing until OK.
#[derive(Clone, Debug)]
struct Draft {
    name: String,
    /// The Page Size tab: a custom size is chosen (and not a named one), the named size chosen, the typed width and height in inches.
    size_custom: bool,
    size_preset: &'static str,
    width: String,
    height: String,
    /// The way the page is held, and whether the person chose it in the dialog (a size typed on its own makes the page the way its sides say).
    orientation: Orientation,
    orientation_chosen: bool,
    /// The printer paper chosen (`None`: the page's own), and the way it is held.
    paper: Option<u32>,
    paper_way: Orientation,
    /// The Drawing Scale tab.
    scale_custom: bool,
    scale_preset: &'static str,
    scale_paper: String,
    scale_paper_unit: &'static str,
    scale_drawing: String,
    scale_drawing_unit: &'static str,
    /// The Page Properties tab: the Type chosen (a background page), and the background page the page is shown over (`None`: no background).
    background: bool,
    back_page: Option<u32>,
}

/// The Page Setup dialog while it is open.
pub struct Dialog {
    pub(crate) tab: DialogTab,
    /// The page the dialog works on: the one on view when it opened.
    pub(crate) page: u32,
    base: PageSetup,
    print: Option<PrintSetup>,
    draft: Draft,
    /// The background pages the Page Properties tab lists for the page's Background: the drawing's, but for the page itself and those that have it for a background.
    backgrounds: Vec<(u32, String)>,
    /// Whether Type may be Background: a drawing keeps a page that is not a background, so the only such page may not be made one.
    can_be_background: bool,
    error: Option<String>,
    /// The tab whose lists have been scrolled to the choice they hold.
    scrolled_for: Option<DialogTab>,
    /// The Layout and Routing tab's draft, read from `page.routing` when the dialog opened (`None`: the engine did not answer).
    routing: Option<routing::Routing>,
    /// The Alt Text tab's draft, read from the page's alt text when the dialog opened (`alt_text.rs`).
    alt: alt_text::Alt,
}

/// The Reorder Pages dialog while it is open.
pub struct Reorder {
    /// The page chosen in the list.
    pub(crate) selected: Option<u32>,
    /// Rename the pages with a default name for the places they take (`page.move`'s `renumber`).
    pub(crate) renumber: bool,
    error: Option<String>,
}

/// What the window remembers for page setup.
#[derive(Default)]
pub struct PageSetupState {
    /// `page.setup` of the page on view, for the revision and the page it was read for. `None` inside: the page has no setup to read.
    view: Option<((u64, u32), Option<PageSetup>)>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) reorder: Option<Reorder>,
}

/// A number the way a text box shows it: whole numbers without a fraction, no more than six places.
pub(crate) fn number_text(n: f64) -> String {
    let rounded = (n * 1.0e6).round() / 1.0e6;
    if rounded.fract() == 0.0 { format!("{}", rounded as i64) } else { format!("{rounded}") }
}

/// A typed number, if it is one: finite and above 0.
fn typed_length(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)
}

/// The unit's spelling in the dialog's lists for an engine unit code (`IN`, `FT`...); inches when the code is no length the engine takes.
fn unit_spelling(code: &str) -> &'static str {
    let unit = length_unit(code).unwrap_or(Unit::Inch);
    UNITS.iter().copied().find(|u| u.eq_ignore_ascii_case(unit.code())).unwrap_or("in")
}

/// A length stored in inches, as it shows in the unit `spelling`.
fn in_unit(inches: f64, spelling: &str) -> f64 {
    length_unit(spelling).unwrap_or(Unit::Inch).from_internal(inches)
}

impl Draft {
    fn of(base: &PageSetup) -> Draft {
        let (paper_value, drawing_value) = (base.scale.paper, base.scale.drawing);
        let paper_unit = unit_spelling(&base.scale.paper_unit);
        let drawing_unit = unit_spelling(&base.scale.drawing_unit);
        Draft {
            name: base.name.clone(),
            size_custom: base.size.is_none(),
            size_preset: base.size.map_or("letter", |s| s.id),
            width: number_text(base.width),
            height: number_text(base.height),
            orientation: base.orientation,
            orientation_chosen: false,
            paper: None,
            paper_way: base.paper.orientation,
            scale_custom: base.scale.preset.is_none(),
            scale_preset: base.scale.preset.map_or("none", |s| s.id),
            scale_paper: number_text(in_unit(paper_value, paper_unit)),
            scale_paper_unit: paper_unit,
            scale_drawing: number_text(in_unit(drawing_value, drawing_unit)),
            scale_drawing_unit: drawing_unit,
            background: base.background,
            back_page: base.back_page,
        }
    }
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "window.pageSetup".to_owned(), msg: msg.into() }
}

impl Dialog {
    /// The commands OK sends: what the person changed, in an order that lets each one meet the page the one before left. A text that is not a number
    /// is a refusal with words.
    fn commands(&self) -> std::result::Result<Vec<(&'static str, Value)>, String> {
        let (base, d) = (&self.base, &self.draft);
        let page = self.page;
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let name = d.name.trim();
        if name.is_empty() {
            return Err("The page needs a name.".to_owned());
        }
        if name != base.name {
            out.push(("page.rename", json!({"id": page, "name": name})));
        }
        // The Page Properties tab's Type and Background: the type first, as a page made a background may name a background of its own.
        if d.background != base.background {
            out.push(("page.type", json!({"type": if d.background { "background" } else { "foreground" }, "page": page})));
        }
        if d.back_page != base.back_page {
            out.push(("page.background", json!({"background": d.back_page, "page": page})));
        }
        // The size, and the way the page is held once it has the size.
        let mut held = base.orientation;
        if d.size_custom {
            let (Some(w), Some(h)) = (typed_length(&d.width), typed_length(&d.height)) else {
                return Err("A custom size needs a width and a height: numbers of inches above 0.".to_owned());
            };
            if (w - base.width).abs() > 1.0e-6 || (h - base.height).abs() > 1.0e-6 {
                out.push(("page.size", json!({"width": w, "height": h, "page": page})));
                held = Orientation::of(w, h);
            }
        } else if base.size.map(|s| s.id) != Some(d.size_preset) {
            out.push(("page.size", json!({"preset": d.size_preset, "page": page})));
        }
        if d.orientation_chosen && d.orientation != held {
            out.push(("page.orientation", json!({"orientation": d.orientation.name(), "page": page})));
        }
        // The drawing scale.
        if d.scale_custom {
            let (Some(paper), Some(drawing)) = (typed_length(&d.scale_paper), typed_length(&d.scale_drawing)) else {
                return Err("A custom scale needs two lengths: numbers above 0.".to_owned());
            };
            let (was_paper, was_drawing) = (
                number_text(in_unit(base.scale.paper, unit_spelling(&base.scale.paper_unit))),
                number_text(in_unit(base.scale.drawing, unit_spelling(&base.scale.drawing_unit))),
            );
            let same = base.scale.preset.is_none()
                && was_paper == number_text(paper)
                && was_drawing == number_text(drawing)
                && unit_spelling(&base.scale.paper_unit) == d.scale_paper_unit
                && unit_spelling(&base.scale.drawing_unit) == d.scale_drawing_unit;
            if !same {
                out.push((
                    "page.scale",
                    json!({"paper": paper, "paper_unit": d.scale_paper_unit, "drawing": drawing, "drawing_unit": d.scale_drawing_unit, "page": page}),
                ));
            }
        } else if base.scale.preset.map(|s| s.id) != Some(d.scale_preset) {
            out.push(("page.scale", json!({"preset": d.scale_preset, "page": page})));
        }
        // The printer paper, last: a named size sets the paper too, and a paper chosen by hand is the person's last word.
        let kind = d.paper.filter(|k| base.paper.kind != Some(*k) || base.paper.assumed);
        let way = (d.paper_way != base.paper.orientation).then_some(d.paper_way);
        if kind.is_some() || way.is_some() {
            let mut params = serde_json::Map::new();
            if let Some(kind) = kind {
                params.insert("kind".to_owned(), json!(kind));
            }
            if let Some(way) = way {
                params.insert("orientation".to_owned(), json!(way.name()));
            }
            params.insert("page".to_owned(), json!(page));
            out.push(("page.paper", Value::Object(params)));
        }
        // The Layout and Routing tab's: a `page.set_routing` and a `connector.line_jumps` with only what was changed.
        if let Some(routing) = &self.routing {
            out.extend(routing.commands(page)?);
        }
        // The Alt Text tab's: a `page.alt_text` with only the text that was changed.
        if let Some(params) = self.alt.command(page) {
            out.push(("page.alt_text", params));
        }
        Ok(out)
    }
}

impl DiacraftApp {
    /// Reads `page.setup` of the page on view if the drawing or the page has changed since it was read. Called before the ribbon is drawn.
    pub(crate) fn refresh_page_view(&mut self) {
        let Some(page) = self.current_page() else {
            self.ui.page_setup.view = None;
            return;
        };
        let key = (self.session.revision(), page);
        if self.ui.page_setup.view.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        let setup = self.session.document().and_then(|doc| read_setup(doc, page).ok());
        self.ui.page_setup.view = Some((key, setup));
    }

    /// What the page on view says of its setup, as the last frame read it.
    pub fn page_view(&self) -> Option<&PageSetup> {
        self.ui.page_setup.view.as_ref().and_then(|(_, setup)| setup.as_ref())
    }

    /// Whether the page on view has Auto Size on.
    pub(crate) fn auto_size_on(&self) -> bool {
        self.page_view().is_some_and(|s| s.auto_size)
    }

    /// Whether the row `mark` of a menu stands for the state the page on view is in.
    pub(crate) fn mark_is_current(&self, mark: crate::ribbon::spec::Mark) -> bool {
        use crate::ribbon::spec::Mark;
        match mark {
            Mark::None => false,
            Mark::Orientation(word) => self.page_view().is_some_and(|s| s.orientation.name() == word),
            Mark::ConnectorStyle(_) | Mark::NoTheme => self.design_mark_is_current(mark),
            // The case the letters of the selection show, and the bullet of its paragraphs: the row of the one they all have is marked.
            Mark::Case(word) => matches!(self.case_mix(), crate::format::Mix::One(case) if case.word() == word),
            Mark::Bullet(n) => matches!(self.bullet_mix(), crate::format::Mix::One(bullet) if bullet == n),
            // The setting the containers selected all have (`container_ui.rs`).
            Mark::ContainerResize(_) | Mark::ContainerHeading(_) | Mark::ContainerMargin(_) => self.container_mark_is_current(mark),
            // The orientation the structure of lanes has (`swimlane_ui.rs`).
            Mark::SwimlaneOrientation(_) => self.swimlane_mark_is_current(mark),
            // The layout the page holds and the arrangement of the position selected (`org_ui.rs`).
            Mark::OrgLayout(_) | Mark::OrgArrangement(_) => self.org_mark_is_current(mark),
        }
    }

    /// Whether the Page Setup dialog is open, and on which tab.
    pub fn page_setup_tab(&self) -> Option<&'static str> {
        self.ui.page_setup.dialog.as_ref().map(|d| d.tab.name())
    }

    /// Whether the Reorder Pages dialog is open.
    pub fn reorder_pages_open(&self) -> bool {
        self.ui.page_setup.reorder.is_some()
    }

    /// Opens the Page Setup dialog on `tab`, for page `page` or the page on view. A dialog that is open changes tab (and keeps what was typed).
    pub(crate) fn open_page_setup(&mut self, tab: DialogTab, page: Option<u32>) -> Result<()> {
        if let Some(dialog) = self.ui.page_setup.dialog.as_mut()
            && page.is_none_or(|p| p == dialog.page)
        {
            dialog.tab = tab;
            return Ok(());
        }
        let page = match page {
            Some(page) => page,
            None => self.current_page().ok_or_else(|| bad("no document open"))?,
        };
        let doc = self.session.document().ok_or_else(|| bad("no document open"))?;
        let base = read_setup(doc, page).map_err(|p| bad(format!("{p:?}")))?;
        let routing = routing::Routing::read(&mut self.session, page, &base.scale.paper_unit);
        let doc = self.session.document().ok_or_else(|| bad("no document open"))?;
        let print = PrintSetup::of(doc, page);
        let draft = Draft::of(&base);
        let alt = alt_text::Alt::read(doc, page);
        // The pages the Background box lists: the background pages, but the page itself and those that would make a loop (a background that has this page for its own).
        let backgrounds: Vec<(u32, String)> = doc
            .pages
            .iter()
            .filter(|p| p.background && p.id != page && !doc.background_chain(p.id).iter().any(|farther| farther.id == page))
            .take(MAX_BACKGROUNDS_LISTED)
            .map(|p| (p.id, shown(&p.name, 40)))
            .collect();
        let can_be_background = base.background || doc.pages.iter().any(|p| p.id != page && !p.background);
        self.ui.page_setup.dialog =
            Some(Dialog { tab, page, base, print, draft, backgrounds, can_be_background, error: None, scrolled_for: None, routing, alt });
        Ok(())
    }

    /// Edit Alt Text for a page: the Title box of the dialog's Alt Text tab takes the keyboard.
    pub(crate) fn page_setup_focus_alt_title(&mut self) {
        if let Some(dialog) = self.ui.page_setup.dialog.as_mut() {
            dialog.alt.focus_title = true;
        }
    }

    /// Closes the Page Setup dialog without sending anything.
    pub(crate) fn close_page_setup(&mut self) {
        self.ui.page_setup.dialog = None;
    }

    /// Forgets the dialogs, what was read of the page and the row of tabs' pointer work: another drawing replaced the one they were about.
    pub(crate) fn forget_page_setup(&mut self) {
        self.ui.page_setup = PageSetupState::default();
        self.ui.text_dialog = None;
        self.ui.snap_dialog = None;
        self.ui.layers = Default::default();
        self.ui.shape_data.define = None;
        self.ui.shape_data.ask = None;
        self.ui.zoom_dialog = None;
        self.ui.offset_dialog = None;
        self.ui.fit_dialog = None;
        self.ui.colors_dialog = None;
        self.ui.palette = None;
        self.ui.relayout = Default::default();
        self.forget_find();
        self.ui.tabs = crate::page_tabs::TabsState::default();
    }

    /// Sends what the dialog changed (OK). On a refusal the dialog stays open and says why.
    fn apply_page_setup(&mut self) {
        let Some(dialog) = self.ui.page_setup.dialog.as_ref() else { return };
        let (page, commands) = (dialog.page, dialog.commands());
        let commands = match commands {
            Ok(commands) => commands,
            Err(words) => {
                if let Some(dialog) = self.ui.page_setup.dialog.as_mut() {
                    dialog.error = Some(words);
                }
                return;
            }
        };
        let refit = commands.iter().any(|(id, _)| matches!(*id, "page.size" | "page.orientation"));
        // A routing command goes in the batch whatever else is sent: the dialog's one undo step is labelled Page setup.
        let routing = commands.iter().any(|(id, _)| matches!(*id, "page.set_routing" | "connector.line_jumps"));
        let result = match commands.as_slice() {
            [] => Ok(()),
            [(id, params)] if !routing => self.execute(id, params).map(|_| ()),
            many => {
                let steps: Vec<Value> = many.iter().map(|(id, params)| json!({"command": id, "params": params})).collect();
                self.execute("command.batch", &json!({"commands": steps, "label": "Page setup"})).map(|_| ())
            }
        };
        match result {
            Ok(()) => {
                self.ui.page_setup.dialog = None;
                if refit && self.ui.page == Some(page) {
                    self.fit_page_to_window();
                }
            }
            Err(e) => {
                if let Some(dialog) = self.ui.page_setup.dialog.as_mut() {
                    dialog.error = Some(e.to_string());
                }
            }
        }
    }

    /// Fits the page on view to the drawing area: after a size or an orientation chosen by hand, so that the whole page is in view.
    pub(crate) fn fit_page_to_window(&mut self) {
        self.fit(false);
    }

    /// The dialogs of page setup over the window: Page Setup and Reorder Pages.
    pub(crate) fn page_dialogs(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.page_setup.dialog.is_some() {
            self.page_setup_dialog(ctx, t);
        }
        if self.ui.page_setup.reorder.is_some() {
            self.reorder_dialog(ctx, t);
        }
        if self.ui.text_dialog.is_some() {
            self.text_dialog(ctx, t);
        }
        if self.ui.snap_dialog.is_some() {
            self.snap_dialog(ctx, t);
        }
        if self.ui.zoom_dialog.is_some() {
            self.zoom_dialog(ctx, t);
        }
        if self.ui.offset_dialog.is_some() {
            self.offset_dialog(ctx, t);
        }
        if self.ui.fit_dialog.is_some() {
            self.fit_curve_dialog(ctx, t);
        }
        if self.ui.colors_dialog.is_some() {
            self.colors_dialog(ctx, t);
        }
        self.relayout_dialogs(ctx, t);
        self.layers_dialogs(ctx, t);
        self.shape_data_dialogs(ctx, t);
        // Not a modal: it floats over the drawing area and the window under it takes the pointer and the keys (`find_dialog.rs`).
        if self.ui.find.dialog.is_some() {
            self.find_dialog_window(ctx, t);
        }
        if self.ui.options_dialog.is_some() {
            self.options_dialog(ctx, t);
        }
        if self.ui.select_by_type.is_some() {
            self.select_by_type_dialog(ctx, t);
        }
        if self.ui.insert.links.is_some() {
            self.links_dialog(ctx, t);
        }
        if self.ui.insert.field.is_some() {
            self.field_dialog(ctx, t);
        }
        if self.ui.insert.symbol.is_some() {
            self.symbol_dialog(ctx, t);
        }
        // The cards of the ShapeSheet window: Insert Section and Edit Formula (`sheet/cards.rs`).
        self.sheet_cards(ctx, t);
        if self.ui.licenses.is_some() {
            self.licenses_window(ctx, t);
        }
    }

    fn page_setup_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 580.0);
        let content_h = (screen.height() - 200.0).clamp(110.0, 380.0);
        // Enter is OK, but not while a box has the keyboard: there Enter ends the typing.
        let typing = ctx.text_edit_focused();
        let Some(mut dialog) = self.ui.page_setup.dialog.take() else { return };
        // While the Spacing dialog is open over this one it takes the keys, and so does a list that is open: Escape closes it, in the frame it is pressed, before the dialog could see
        // it, so whether one was open is asked first.
        let spacing_open = dialog.routing.as_ref().is_some_and(|r| r.dialog.is_some());
        let list_open = egui::Popup::is_any_open(ctx);
        let mut ok = false;
        let mut cancel = false;
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("page_setup_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Page Setup").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            tab_strip(ui, &t, &mut dialog, &mut seen);
            ui.add_space(6.0);
            // **The content is as tall for every tab**, and so is each list in it: a dialog that changes its size as its parts find theirs takes
            // several frames to settle in the middle of the window (and a scroll area left to shrink stays as small as it first saw its rows).
            let list_h = (content_h * 0.5).clamp(70.0, 190.0);
            let content = egui::ScrollArea::vertical()
                .id_salt("page_setup_content")
                .min_scrolled_height(content_h)
                .max_height(content_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(width - 14.0);
                    match dialog.tab {
                        DialogTab::PrintSetup => print_tab(ui, &t, &mut dialog, list_h, &mut seen),
                        DialogTab::PageSize => size_tab(ui, &t, &mut dialog, list_h, &mut seen),
                        DialogTab::DrawingScale => scale_tab(ui, &t, &mut dialog, list_h, &mut seen),
                        DialogTab::PageProperties => properties_tab(ui, &t, &mut dialog, &mut seen),
                        DialogTab::LayoutAndRouting => routing::tab(ui, &t, &mut dialog, &mut seen),
                        DialogTab::AltText => alt_text::tab(ui, &t, &mut dialog, &mut seen),
                    }
                });
            seen.push(("pagesetup.content".to_owned(), content.inner_rect, true));
            ui.add_space(6.0);
            if let Some(error) = &dialog.error {
                ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "pagesetup.ok", true), ("Cancel", "pagesetup.cancel", false)] {
                    let button = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(egui::vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), button.rect, true));
                    if button.clicked() {
                        if primary { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !list_open && !spacing_open {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                cancel = true;
            }
            if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                ok = true;
            }
        }
        self.ui.page_setup.dialog = Some(dialog);
        if cancel {
            self.close_page_setup();
        } else if ok {
            self.apply_page_setup();
        }
        // The Spacing dialog is drawn last, so that it is the one on top.
        self.routing_spacing_dialog(ctx, t);
    }

    /// What the window reports of the Layout and Routing tab's draft, its Spacing dialog and its preview while the Page Setup dialog is open; `null` otherwise.
    pub(crate) fn page_setup_routing_state(&self) -> Value {
        self.ui.page_setup.dialog.as_ref().and_then(|d| d.routing.as_ref()).map_or(Value::Null, routing::Routing::state_json)
    }
}

/// The row of tabs.
fn tab_strip(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.horizontal_wrapped(|ui| {
        for tab in DialogTab::ALL {
            let selected = dialog.tab == tab;
            let button = ui.add(egui::Button::selectable(selected, RichText::new(tab.name()).font(theme::font(size::LABEL)).color(t.text)));
            seen.push((format!("pagesetup.tab.{}", tab.name()), button.rect, true));
            if button.clicked() {
                dialog.tab = tab;
            }
        }
    });
}

fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
}

fn label(ui: &mut Ui, t: &Tokens, words: impl Into<String>) {
    ui.label(RichText::new(words.into()).font(theme::font(size::LABEL)).color(t.text));
}

/// A small box for a typed number.
fn field(ui: &mut Ui, text: &mut String, id: &str, enabled: bool, seen: &mut Vec<(String, egui::Rect, bool)>) -> egui::Response {
    let response =
        ui.add_enabled(enabled, egui::TextEdit::singleline(text).desired_width(64.0).char_limit(FIELD_CHARS).font(theme::font(size::LABEL)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, enabled, id));
    seen.push((id.to_owned(), response.rect, enabled));
    response
}

/// The two radio buttons of a way the page or the paper is held.
fn orientation_choice(ui: &mut Ui, t: &Tokens, way: &mut Orientation, prefix: &str, seen: &mut Vec<(String, egui::Rect, bool)>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (choice, words) in [(Orientation::Portrait, "Portrait"), (Orientation::Landscape, "Landscape")] {
            let r = ui.radio(*way == choice, RichText::new(words).font(theme::font(size::LABEL)).color(t.text));
            seen.push((format!("{prefix}.{}", choice.name()), r.rect, true));
            if r.clicked() && *way != choice {
                *way = choice;
                changed = true;
            }
        }
    });
    changed
}

/// A size on paper as the lists write it: inches, or millimetres for the ISO sizes.
fn size_words(width: f64, height: f64, metric: bool) -> String {
    if metric {
        format!("{} x {} mm", number_text((width * 25.4).round()), number_text((height * 25.4).round()))
    } else {
        format!("{} x {} in", number_text(width), number_text(height))
    }
}

/// A list row that fills the list's width and keeps its text at the left: the row of the choice made is marked.
fn list_row(ui: &mut Ui, t: &Tokens, selected: bool, words: &str) -> egui::Response {
    let row = egui::Button::selectable(selected, RichText::new(words).font(theme::font(size::LABEL)).color(t.text))
        .min_size(egui::vec2(ui.available_width(), 22.0));
    ui.add(row)
}

/// A box that shows a number the page's cells hold and takes none: the engine has no command that writes it.
fn quiet_box(ui: &mut Ui, text: &str, words: &str) -> egui::Response {
    let mut shown = text.to_owned();
    let response = ui.add(egui::TextEdit::singleline(&mut shown).desired_width(44.0).font(theme::font(size::LABEL)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, false, words));
    response
}

fn print_tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, list_h: f32, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Printer paper");
    let scroll_now = dialog.scrolled_for != Some(DialogTab::PrintSetup);
    let current = dialog.draft.paper.or(if dialog.base.paper.assumed { None } else { dialog.base.paper.kind });
    let list_h = (list_h * 0.8).max(60.0);
    let list =
        egui::ScrollArea::vertical().id_salt("page_setup_papers").min_scrolled_height(list_h).max_height(list_h).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for paper in PAPERS {
                        let selected = current == Some(paper.kind);
                        let metric = paper.name.starts_with('A') || paper.name.contains("JIS");
                        let words = format!("{}   {}", paper.name, size_words(paper.width, paper.height, metric));
                        let r = list_row(ui, t, selected, &words);
                        seen.push((format!("pagesetup.paper.{}", paper.kind), r.rect, true));
                        if selected && scroll_now {
                            r.scroll_to_me(Some(egui::Align::Center));
                        }
                        if r.clicked() {
                            dialog.draft.paper = Some(paper.kind);
                        }
                    }
                });
            },
        );
    seen.push(("pagesetup.paper.list".to_owned(), list.inner_rect, true));
    if dialog.base.paper.assumed && dialog.draft.paper.is_none() {
        ui.label(RichText::new("This page names no paper, so Letter is assumed.").font(theme::font(size::LABEL)).color(t.text_dim));
    }
    dialog.scrolled_for = Some(DialogTab::PrintSetup);
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        label(ui, t, "Paper is held");
        orientation_choice(ui, t, &mut dialog.draft.paper_way, "pagesetup.paper_orientation", seen);
    });
    ui.add_space(4.0);
    // What the page's cells say about printing, drawn quiet: the engine has no command that writes them yet.
    heading(ui, t, "Print zoom, margins and centring: the page's own cells, which no command changes yet");
    let print = dialog.print;
    ui.add_enabled_ui(false, |ui| {
        ui.horizontal_wrapped(|ui| {
            let r = ui.radio(print.is_some_and(|p| !p.fit), "Adjust to");
            seen.push(("pagesetup.print.zoom".to_owned(), r.rect, false));
            quiet_box(ui, &number_text(print.map_or(100.0, |p| p.scale_x * 100.0)), "Print zoom in percent");
            ui.label("% of normal size");
        });
        ui.horizontal_wrapped(|ui| {
            let r = ui.radio(print.is_some_and(|p| p.fit), "Fit to");
            seen.push(("pagesetup.print.fit".to_owned(), r.rect, false));
            quiet_box(ui, &print.map_or(1, |p| p.pages_x).to_string(), "Sheets across");
            ui.label("sheets across by");
            quiet_box(ui, &print.map_or(1, |p| p.pages_y).to_string(), "Sheets down");
            ui.label("down");
        });
        let (left, right, top, bottom) = print.map_or((0.25, 0.25, 0.25, 0.25), |p| (p.left, p.right, p.top, p.bottom));
        let margins = ui.horizontal_wrapped(|ui| {
            ui.label("Margins in inches: left");
            quiet_box(ui, &number_text(left), "Left margin");
            ui.label("right");
            quiet_box(ui, &number_text(right), "Right margin");
            ui.label("top");
            quiet_box(ui, &number_text(top), "Top margin");
            ui.label("bottom");
            quiet_box(ui, &number_text(bottom), "Bottom margin");
        });
        seen.push(("pagesetup.print.margins".to_owned(), margins.response.rect, false));
        ui.horizontal_wrapped(|ui| {
            for (name, words, on) in [
                ("centre_x", "Center horizontally", print.is_some_and(|p| p.centre_x)),
                ("centre_y", "Center vertically", print.is_some_and(|p| p.centre_y)),
                ("grid", "Print grid lines", print.is_some_and(|p| p.print_grid)),
            ] {
                let mut on = on;
                let r = ui.checkbox(&mut on, words);
                seen.push((format!("pagesetup.print.{name}"), r.rect, false));
            }
        });
    });
}

fn size_tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, list_h: f32, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let scroll_now = dialog.scrolled_for != Some(DialogTab::PageSize);
    let r = ui.radio(!dialog.draft.size_custom, RichText::new("Pre-defined size").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("pagesetup.size.predefined".to_owned(), r.rect, true));
    if r.clicked() {
        dialog.draft.size_custom = false;
    }
    let list =
        egui::ScrollArea::vertical().id_salt("page_setup_sizes").min_scrolled_height(list_h).max_height(list_h).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    let mut group = "";
                    for preset in SIZES {
                        if preset.group != group {
                            group = preset.group;
                            heading(ui, t, group);
                        }
                        let selected = !dialog.draft.size_custom && dialog.draft.size_preset == preset.id;
                        let metric = preset.group == "ISO";
                        let words = format!("{}   {}", preset.name, size_words(preset.width, preset.height, metric));
                        let r = list_row(ui, t, selected, &words);
                        seen.push((format!("pagesetup.size.{}", preset.id), r.rect, true));
                        if selected && scroll_now {
                            r.scroll_to_me(Some(egui::Align::Center));
                        }
                        if r.clicked() {
                            dialog.draft.size_preset = preset.id;
                            dialog.draft.size_custom = false;
                        }
                    }
                });
            },
        );
    seen.push(("pagesetup.size.list".to_owned(), list.inner_rect, true));
    dialog.scrolled_for = Some(DialogTab::PageSize);
    ui.add_space(4.0);
    let r = ui.radio(dialog.draft.size_custom, RichText::new("Custom size").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("pagesetup.size.custom".to_owned(), r.rect, true));
    if r.clicked() {
        dialog.draft.size_custom = true;
    }
    ui.horizontal_wrapped(|ui| {
        label(ui, t, "Width");
        if field(ui, &mut dialog.draft.width, "pagesetup.size.width", true, seen).changed() {
            dialog.draft.size_custom = true;
        }
        label(ui, t, "Height");
        if field(ui, &mut dialog.draft.height, "pagesetup.size.height", true, seen).changed() {
            dialog.draft.size_custom = true;
        }
        label(ui, t, "in");
    });
    ui.add_space(6.0);
    heading(ui, t, "Page orientation");
    if orientation_choice(ui, t, &mut dialog.draft.orientation, "pagesetup.orientation", seen) {
        // The page turns with the sizes typed: the width and the height change places.
        std::mem::swap(&mut dialog.draft.width, &mut dialog.draft.height);
        dialog.draft.orientation_chosen = true;
    }
    ui.add_space(4.0);
    label(
        ui,
        t,
        format!(
            "The page is {} x {} in on paper; {} x {} sheets of {} cover it.",
            number_text(dialog.base.width),
            number_text(dialog.base.height),
            dialog.base.sheets.0,
            dialog.base.sheets.1,
            dialog.base.paper.known.map_or("the assumed paper", |p| p.name)
        ),
    );
}

fn scale_tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, list_h: f32, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let scroll_now = dialog.scrolled_for != Some(DialogTab::DrawingScale);
    let r = ui.radio(!dialog.draft.scale_custom, RichText::new("Pre-defined scale").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("pagesetup.scale.predefined".to_owned(), r.rect, true));
    if r.clicked() {
        dialog.draft.scale_custom = false;
    }
    let list =
        egui::ScrollArea::vertical().id_salt("page_setup_scales").min_scrolled_height(list_h).max_height(list_h).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    let mut family = u8::MAX;
                    for preset in SCALES {
                        if preset.kind != family {
                            family = preset.kind;
                            // The scales of no family (1:1) have no heading; the others have their family's name with a capital.
                            if preset.kind != 0 {
                                let name = diacraft_engine::draw::paper::scale_family(preset.kind);
                                let mut chars = name.chars();
                                let capital: String = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
                                heading(ui, t, &capital);
                            }
                        }
                        let selected = !dialog.draft.scale_custom && dialog.draft.scale_preset == preset.id;
                        let r = list_row(ui, t, selected, preset.name);
                        seen.push((format!("pagesetup.scale.{}", preset.id), r.rect, true));
                        if selected && scroll_now {
                            r.scroll_to_me(Some(egui::Align::Center));
                        }
                        if r.clicked() {
                            dialog.draft.scale_preset = preset.id;
                            dialog.draft.scale_custom = false;
                        }
                    }
                });
            },
        );
    seen.push(("pagesetup.scale.list".to_owned(), list.inner_rect, true));
    dialog.scrolled_for = Some(DialogTab::DrawingScale);
    ui.add_space(4.0);
    let r = ui.radio(dialog.draft.scale_custom, RichText::new("Custom scale").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("pagesetup.scale.custom".to_owned(), r.rect, true));
    if r.clicked() {
        dialog.draft.scale_custom = true;
    }
    ui.horizontal_wrapped(|ui| {
        if field(ui, &mut dialog.draft.scale_paper, "pagesetup.scale.paper", true, seen).changed() {
            dialog.draft.scale_custom = true;
        }
        unit_choice(ui, &mut dialog.draft.scale_paper_unit, "pagesetup.scale.paper_unit", seen);
        label(ui, t, "on paper =");
        if field(ui, &mut dialog.draft.scale_drawing, "pagesetup.scale.drawing", true, seen).changed() {
            dialog.draft.scale_custom = true;
        }
        unit_choice(ui, &mut dialog.draft.scale_drawing_unit, "pagesetup.scale.drawing_unit", seen);
        label(ui, t, "in the drawing");
    });
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            RichText::new("A new scale keeps the page's size on paper. The shapes stay as they are.")
                .font(theme::font(size::LABEL))
                .color(t.text_dim),
        )
        .wrap(),
    );
}

/// A list of the units a custom scale is given in.
fn unit_choice(ui: &mut Ui, unit: &mut &'static str, id: &str, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let response = egui::ComboBox::from_id_salt(id).width(52.0).selected_text(*unit).show_ui(ui, |ui| {
        for choice in UNITS {
            ui.selectable_value(unit, choice, choice);
        }
    });
    let words = if id.ends_with("paper_unit") { "Unit of the length on paper" } else { "Unit of the length in the drawing" };
    response.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, words));
    seen.push((id.to_owned(), response.response.rect, true));
}

fn properties_tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.horizontal(|ui| {
        label(ui, t, "Name");
        let response =
            ui.add(egui::TextEdit::singleline(&mut dialog.draft.name).desired_width(220.0).char_limit(NAME_CHARS).font(theme::font(size::LABEL)));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Page name"));
        seen.push(("pagesetup.name".to_owned(), response.rect, true));
    });
    ui.add_space(4.0);
    // Type: Foreground or Background. A drawing keeps a page that is not a background, so the only one may not be made one.
    ui.horizontal(|ui| {
        label(ui, t, "Type");
        let foreground = ui.radio(!dialog.draft.background, RichText::new("Foreground").font(theme::font(size::LABEL)).color(t.text));
        seen.push(("pagesetup.type.foreground".to_owned(), foreground.rect, true));
        if foreground.clicked() {
            dialog.draft.background = false;
        }
        let can = dialog.can_be_background;
        let background = ui.add_enabled(
            can,
            egui::RadioButton::new(
                dialog.draft.background,
                RichText::new("Background").font(theme::font(size::LABEL)).color(if can { t.text } else { t.text_disabled }),
            ),
        );
        seen.push(("pagesetup.type.background".to_owned(), background.rect, can));
        if !can {
            background.clone().on_disabled_hover_text("A drawing keeps one page that is not a background.");
        }
        if background.clicked() {
            dialog.draft.background = true;
        }
    });
    ui.add_space(4.0);
    // Background: None, or one of the drawing's background pages, shown behind this page's shapes. With no background page the only item is None.
    ui.horizontal(|ui| {
        label(ui, t, "Background");
        let shown_name = match dialog.draft.back_page {
            None => "None".to_owned(),
            Some(id) => dialog.backgrounds.iter().find(|(b, _)| *b == id).map_or_else(|| format!("Page {id}"), |(_, name)| name.clone()),
        };
        let combo = egui::ComboBox::from_id_salt("pagesetup.background").width(200.0).selected_text(shown_name).show_ui(ui, |ui| {
            ui.selectable_value(&mut dialog.draft.back_page, None, "None");
            for (id, name) in &dialog.backgrounds {
                ui.selectable_value(&mut dialog.draft.back_page, Some(*id), name.as_str());
            }
        });
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Background page"));
        seen.push(("pagesetup.background".to_owned(), combo.response.rect, true));
    });
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            RichText::new("A page name is kept as typed; the page tab shows it. A background page is shown behind the shapes of the pages that name it, and its own shapes are changed on its own tab.")
                .font(theme::font(size::LABEL))
                .color(t.text_dim),
        )
        .wrap(),
    );
}

// ------------------------------------------------------------------------------------------------------------------ Reorder Pages

impl DiacraftApp {
    /// Opens the Reorder Pages dialog (`page.move`).
    pub(crate) fn open_reorder_pages(&mut self) {
        if self.ui.page_setup.reorder.is_none() {
            let selected = self.current_page();
            self.ui.page_setup.reorder = Some(Reorder { selected, renumber: false, error: None });
        }
    }

    /// The pages the tabs show in order, without the backgrounds: what `page.move` counts places in.
    fn foreground_pages(&self) -> Vec<(u32, String)> {
        self.session.document().map_or_else(Vec::new, |doc| doc.pages.iter().filter(|p| !p.background).map(|p| (p.id, shown(&p.name, 40))).collect())
    }

    fn reorder_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(260.0, 380.0);
        let pages = self.foreground_pages();
        if pages.len() < 2 {
            // A page was deleted under the dialog (undo, the control channel): nothing is left to put in order.
            self.ui.page_setup.reorder = None;
            return;
        }
        let Some(mut state) = self.ui.page_setup.reorder.take() else { return };
        if !state.selected.is_some_and(|s| pages.iter().any(|(id, _)| *id == s)) {
            state.selected = pages.first().map(|(id, _)| *id);
        }
        let place = pages.iter().position(|(id, _)| Some(*id) == state.selected).unwrap_or(0);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let mut moves: Option<usize> = None;
        let mut close = false;
        let modal = egui::Modal::new(Id::unique("reorder_pages_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Reorder Pages").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            heading(ui, &t, "Pages, in the order of the tabs");
            egui::ScrollArea::vertical().id_salt("reorder_list").min_scrolled_height(180.0).max_height(180.0).auto_shrink([false, false]).show(
                ui,
                |ui| {
                    for (id, name) in &pages {
                        let r = ui.selectable_label(state.selected == Some(*id), RichText::new(name).font(theme::font(size::LABEL)).color(t.text));
                        seen.push((format!("reorder.page.{id}"), r.rect, true));
                        if r.clicked() {
                            state.selected = Some(*id);
                        }
                    }
                },
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let up = ui.add_enabled(
                    place > 0,
                    egui::Button::new(RichText::new("Move Up").font(theme::font(size::LABEL))).min_size(egui::vec2(84.0, 26.0)),
                );
                seen.push(("reorder.up".to_owned(), up.rect, place > 0));
                if up.clicked() {
                    moves = Some(place.saturating_sub(1));
                }
                let can_down = place + 1 < pages.len();
                let down = ui.add_enabled(
                    can_down,
                    egui::Button::new(RichText::new("Move Down").font(theme::font(size::LABEL))).min_size(egui::vec2(84.0, 26.0)),
                );
                seen.push(("reorder.down".to_owned(), down.rect, can_down));
                if down.clicked() {
                    moves = Some(place + 1);
                }
            });
            let renumber =
                ui.checkbox(&mut state.renumber, RichText::new("Update page names to match the order").font(theme::font(size::LABEL)).color(t.text));
            seen.push(("reorder.renumber".to_owned(), renumber.rect, true));
            if let Some(error) = &state.error {
                ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
            }
            ui.add_space(6.0);
            let button = ui.add(egui::Button::new(RichText::new("Close").font(theme::font(size::LABEL))).min_size(egui::vec2(84.0, 26.0)));
            seen.push(("reorder.close".to_owned(), button.rect, true));
            if button.clicked() {
                close = true;
            }
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            close = true;
        }
        if let (Some(to), Some(id)) = (moves, state.selected) {
            state.error = self.execute("page.move", &json!({"id": id, "to": to, "renumber": state.renumber})).err().map(|e| e.to_string());
        }
        if !close {
            self.ui.page_setup.reorder = Some(state);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ Auto Size

/// The commands that put shapes on a page or move them, and so can make the page grow when it has Auto Size on (`docs/editing-commands.md`,
/// "Where it hooks").
const GROWS: [&str; 9] =
    ["shape.drop", "shape.move", "shape.align", "shape.resize", "shape.rotate", "shape.flip", "edit.paste", "shape.duplicate", "text.box"];

/// The most shapes whose places are remembered to find out how far a growth carried the drawing.
const MAX_REFERENCES: usize = 16;

/// What a command that may grow the page needs to remember: the page's size and where a few shapes it does not touch were.
pub(crate) struct Watch {
    page: u32,
    size: (f64, f64),
    references: Vec<(u32, (f64, f64))>,
}

impl DiacraftApp {
    /// Before a command that may make the page grow. **Only a page that grows by itself (Auto Size) and is looked at by a view of its own**
    /// (not one that follows Fit, which shows the whole page whatever it grows to) needs it: `None` for any other command or page.
    pub(crate) fn watch_growth(&mut self, id: &str, params: &Value) -> Option<Watch> {
        if !GROWS.contains(&id) {
            return None;
        }
        let page = params.get("page").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).or(self.ui.page)?;
        self.ui.views.get(&page).filter(|view| !view.fitted)?;
        let setup = self.session.document().and_then(|doc| read_setup(doc, page).ok()).filter(|setup| setup.auto_size)?;
        let named: Vec<u64> = params
            .get("ids")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_u64).collect())
            .or_else(|| params.get("id").and_then(Value::as_u64).map(|id| vec![id]))
            .unwrap_or_default();
        if !self.ensure_scene(page) {
            return None;
        }
        let scene = self.ui.edit.scene()?;
        let references = scene
            .items
            .iter()
            .filter(|item| item.kind != crate::edit::scene::Kind::Connector && !named.contains(&u64::from(item.id)))
            .take(MAX_REFERENCES)
            .map(|item| (item.id, item.pin))
            .collect();
        Some(Watch { page, size: (setup.width, setup.height), references })
    }

    /// After the command: the drawing is read again (the scene and the picture follow the revision), and **when the page grew on its left or its
    /// bottom the view follows the drawing**: the engine carried every shape, so a shape the command did not touch is the measure of how far, and the
    /// view's middle moves by as much, so that nothing jumps on the screen. A page that grew right or up has carried nothing.
    pub(crate) fn settle_growth(&mut self, watch: Watch) {
        let Some(now) = self.session.document().and_then(|doc| read_setup(doc, watch.page).ok()) else { return };
        if (now.width - watch.size.0).abs() < 1e-9 && (now.height - watch.size.1).abs() < 1e-9 {
            return;
        }
        if !self.ensure_scene(watch.page) {
            return;
        }
        let carried = self
            .ui
            .edit
            .scene()
            .and_then(|scene| watch.references.iter().find_map(|(id, was)| scene.get(*id).map(|item| (item.pin.0 - was.0, item.pin.1 - was.1))));
        if let (Some((dx, dy)), Some(view)) = (carried, self.ui.views.get_mut(&watch.page))
            && dx.is_finite()
            && dy.is_finite()
        {
            view.center = (view.center.0 + dx, view.center.1 + dy);
        }
    }
}
