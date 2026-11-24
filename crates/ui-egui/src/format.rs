//! Formatting from the window: what the selection looks like, for the controls of the Home tab, and the window's commands that format it.
//!
//! **The controls show the selection's state** (`shape.formatting`, `docs/editing-commands.md`): [`FormatView`] holds what the cells of each
//! selected shape resolve to, read once for each revision of the drawing and each selection, and a control asks it whether the shapes agree.
//! The engine answers for each shape; it has no summary for a selection of several, so **what a mixed selection shows is the window's rule,
//! ** (ours): a control that shows a value shows it when every shape has it and shows nothing when they differ (a blank Font box, a
//! bar with a slash through it); a toggle is pressed when every shape has it, half pressed when some do, and a press on a toggle that is not
//! pressed in full turns it on for every shape.
//!
//! **A text box that is open reads its own characters** (gap U19 closed): with a box open the Font group shows what the characters selected in
//! it are, or the run before the caret when nothing is selected, as `shape.formatting` answers with `start` and `end`: each
//! property is the one value its runs share or mixed. A toggle is pressed, not pressed or half pressed (mixed); the Font and Size boxes show the value
//! or nothing; the Font Color bar shows the colour or the slash; and a press on a mixed toggle turns it on (again).
//!
//! **Every change is a command** (`docs/editing-commands.md`). The window's `format.*` commands act on the selection and send one engine
//! command, `shape.format` or `text.format`, with the ids and the page; Fill and Line colours, weights, dashes and line ends go the first
//! way, and fonts, sizes, styles, colours and alignment the second. **A text box that is open**: `format.text` and the commands
//! built on it send what was typed first, send `text.format` with `start` and `end` for the characters selected in the box (a caret and no
//! selection formats the whole text, as the inventory has it: "the whole block when only clicked"), and open the box again with the same
//! selection, so that the person goes on typing.

use diacraft_engine::formatting::{Align, Case, Direction, MAX_SIZE_POINTS, RangeLook, Rgb, ShapeLook, VAlign, look_of, look_of_range};
use diacraft_engine::{EngineError, Result, shown};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::font_list::FontList;
use crate::ribbon::spec::Press;

/// The selection as the page on view has it (`DiacraftApp::selection_on_view`).
pub(crate) enum OnView {
    /// No document is open.
    NoDocument,
    /// Nothing is selected.
    Nothing,
    /// Shapes are selected, on another page than the one on view.
    Elsewhere,
    /// The page on view, and the shapes selected on it.
    Here(u32, Vec<u32>),
}

/// What a command that acts on the selection says when the shapes selected are not on the page on view (`selection_target`).
pub(crate) const SELECTION_ELSEWHERE: &str =
    "the shapes selected are on another page than the one shown: select the shapes on the page shown, or show the page they are on";

/// The most shapes of a selection whose formatting the controls read: a selection of the whole of a large page is summarised by its first
/// shapes, so that choosing it does not hold a frame for the time the engine takes to read thousands of shapes.
pub const MAX_LOOKED_AT: usize = 1_000;

/// The sizes the Size box lists and the Increase and Decrease buttons step through, in points. **Our rule (no specification gives the
/// steps)**: a step goes to the next size of the list above the size of the shape selected first (below, for Decrease), and the same size goes
/// to every shape selected. Past the list the step is a point down, to 1, and 12 points up, to the most a size can be.
pub const FONT_SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0, 60.0, 72.0];

/// The size a step from `current` gives, in points.
pub fn step_size(current: f64, up: bool) -> f64 {
    let at = if current.is_finite() { (current * 100.0).round() / 100.0 } else { 12.0 };
    let stepped = if up {
        FONT_SIZES.iter().copied().find(|s| *s > at + 0.005).unwrap_or(at + 12.0)
    } else {
        FONT_SIZES.iter().rev().copied().find(|s| *s < at - 0.005).unwrap_or(at - 1.0)
    };
    stepped.clamp(1.0, MAX_SIZE_POINTS)
}

/// The `text.format` parameter a toggle sets, and its value when the toggle goes `on`: the style flags are booleans, Bullets is the first bullet or none,
/// Vertical Text is a boolean, and the two cases are their own or normal.
fn toggle_param(what: &str, on: bool) -> (&str, Value) {
    match what {
        "bullets" => ("bullet", json!(u8::from(on))),
        "vertical" => ("vertical", json!(on)),
        "all_caps" => ("case", json!(if on { "all_caps" } else { "normal" })),
        "small_caps" => ("case", json!(if on { "small_caps" } else { "normal" })),
        other => (other, json!(on)),
    }
}

/// What the shapes of a selection have in common.
#[derive(Clone, Debug, PartialEq)]
pub enum Mix<T> {
    /// Nothing is selected.
    Nothing,
    /// Every shape has this.
    One(T),
    /// The shapes differ.
    Mixed,
}

/// How far a toggle is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    Off,
    /// Some of the shapes have it.
    Part,
    On,
}

/// The formatting of the selected shapes at one revision of the drawing.
pub struct FormatView {
    /// The drawing's generation and revision, the page, the shapes and **whether the faces of the machine were in hand** (`fonts_ready`) it was read for.
    key: (u64, u64, u32, Vec<u32>, bool),
    /// One for each of the first [`MAX_LOOKED_AT`] shapes selected, in the order selected.
    pub looks: Vec<ShapeLook>,
}

impl FormatView {
    /// What `of` gives for every shape, when it is the same.
    pub fn mix<T: PartialEq>(&self, of: impl Fn(&ShapeLook) -> T) -> Mix<T> {
        let mut all = self.looks.iter().map(of);
        let Some(first) = all.next() else { return Mix::Nothing };
        if all.all(|other| other == first) { Mix::One(first) } else { Mix::Mixed }
    }

    /// How far `has` is true of the shapes.
    pub fn pressed(&self, has: impl Fn(&ShapeLook) -> bool) -> Pressed {
        let on = self.looks.iter().filter(|look| has(look)).count();
        if on == 0 {
            Pressed::Off
        } else if on == self.looks.len() {
            Pressed::On
        } else {
            Pressed::Part
        }
    }
}

/// Whether the faces of the machine are in hand, which is what decides the face `shape.formatting` names for text whose cells name none: the default of the
/// font database of the application, once its scan has ended, and the bundled `Ubuntu` before (and where nothing is shared). A view read before the scan ended is
/// not the view read after it, so this is part of the key of every view. Never waits.
fn fonts_ready() -> bool {
    diacraft_engine::measure::measure_fonts_now().is_some()
}

impl FormatState {
    /// A drawing replaced the open one: what was read of the old one is not read again.
    pub(crate) fn forget_looks(&mut self) {
        self.view = None;
        self.range = None;
    }
}

/// What the window remembers for the formatting controls.
#[derive(Default)]
pub struct FormatState {
    view: Option<FormatView>,
    /// The faces the application is finding (`DiacraftApp::with_fonts`): while they are not in hand the window asks for a frame soon, so that the face the Font box
    /// names is read again when they are, with no edit and no change of selection.
    pub(crate) shared: Option<diacraft_engine::measure::SharedFonts>,
    /// What the characters selected in the text box that is open (or the run before its caret) are, and what it was read for.
    range: Option<(RangeKey, RangeLook)>,
    /// The machine's font families, found on a thread of their own the first time the Font box opens.
    pub fonts: FontList,
    /// The pass in which the Font list and the Size list were last drawn: one not drawn for a while has just opened (`ribbon/pickers.rs`).
    pub menu_seen: [u64; 2],
    /// What was typed in the box of the list that is open.
    pub filter: String,
}

/// What the state of a range was read for: the revision, the page, the shape and the range.
type RangeKey = (u64, u64, u32, u32, usize, usize, bool);

/// A weight in thousandths of a point, for comparing weights that were stored as inches.
pub(crate) fn thousandths(points: f64) -> i64 {
    (points * 1000.0).round() as i64
}

/// How far `press` is on over the characters a range shows: pressed or not when its runs agree, half pressed when they differ (a mixed toggle),
/// and for alignment the button of the paragraph's when the paragraphs agree and none when they differ. `None` for a press that is not of the
/// characters or the paragraph (the vertical alignment is the shape's, and is read from the shape).
fn range_pressed(range: &RangeLook, press: Press) -> Option<Pressed> {
    let toggle = |mixed: bool, on: bool| {
        if mixed {
            Pressed::Part
        } else if on {
            Pressed::On
        } else {
            Pressed::Off
        }
    };
    let aligned = |to: Align| if !range.mixed.align && range.look.align == to { Pressed::On } else { Pressed::Off };
    match press {
        Press::Bold => Some(toggle(range.mixed.bold, range.look.bold)),
        Press::Italic => Some(toggle(range.mixed.italic, range.look.italic)),
        Press::Underline => Some(toggle(range.mixed.underline, range.look.underline)),
        Press::Strike => Some(toggle(range.mixed.strike, range.look.strike)),
        Press::Left => Some(aligned(Align::Left)),
        Press::Center => Some(aligned(Align::Center)),
        Press::Right => Some(aligned(Align::Right)),
        Press::Justify => Some(aligned(Align::Justify)),
        Press::Distributed => Some(aligned(Align::Distributed)),
        Press::Bullets => Some(toggle(range.mixed.bullet, range.look.bullet != 0)),
        Press::Ltr => Some(toggle(range.mixed.direction, range.look.direction == Direction::LeftToRight)),
        Press::Rtl => Some(toggle(range.mixed.direction, range.look.direction == Direction::RightToLeft)),
        Press::AllCaps => Some(toggle(range.mixed.case, range.look.case == Case::AllCaps)),
        Press::SmallCaps => Some(toggle(range.mixed.case, range.look.case == Case::SmallCaps)),
        // The block's `TextDirection` is the shape's, whatever the characters selected.
        Press::None | Press::Top | Press::Middle | Press::Bottom | Press::Vertical | Press::AutoSize => None,
    }
}

impl DiacraftApp {
    /// Reads the formatting of the selection if the drawing or the selection has changed since it was read. Called before the ribbon is drawn.
    pub(crate) fn refresh_format_view(&mut self) {
        self.refresh_range();
        // The formatting of the selection is read for the page on view only when the selection is of it (`selection_on_view`): the controls and the Format Shape
        // pane never show another page's values.
        let Some((page, mut ids)) = self.selection_here() else {
            self.ui.format.view = None;
            return;
        };
        ids.truncate(MAX_LOOKED_AT);
        let key = (self.ui.generation, self.session.revision(), page, ids, fonts_ready());
        if self.ui.format.view.as_ref().is_some_and(|view| view.key == key) {
            return;
        }
        // A refusal (a shape that is not on the page: the selection is pruned a frame later) leaves the controls with nothing to show for one frame.
        let looks = self.session.document().and_then(|doc| look_of(doc, key.2, &key.3).ok()).unwrap_or_default();
        self.ui.format.view = Some(FormatView { key, looks });
    }

    /// Reads what the characters selected in the open text box are (the run before the caret, for a caret), as `shape.formatting` answers with `start` and
    /// `end`. Kept for the revision and the range it was read for. **While characters typed have not been sent** the drawing has not
    /// the characters the offsets count, so the state read last is kept as it is (typing joins the run before the caret and changes no run).
    fn refresh_range(&mut self) {
        let Some(open) = self.open_text_selection() else {
            self.ui.format.range = None;
            return;
        };
        let Some((from, to)) = open.range else {
            self.ui.format.range = None;
            return;
        };
        if self.has_unsent_text() {
            return;
        }
        let key = (self.ui.generation, self.session.revision(), open.page, open.id, from, to, fonts_ready());
        if self.ui.format.range.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        self.ui.format.range = self.read_range(open.page, open.id, from, to).map(|look| (key, look));
    }

    /// What `shape.formatting` answers for the characters `from..to` of shape `id` (a point when they are equal); `None` where it refuses (offsets
    /// that the text does not have, a shape that is gone).
    fn read_range(&self, page: u32, id: u32, from: usize, to: usize) -> Option<RangeLook> {
        self.session.document().and_then(|doc| look_of_range(doc, page, &[id], from, to).ok())
    }

    /// What the selected shapes are formatted as, as the last frame read it. `None` with nothing selected.
    pub fn format_view(&self) -> Option<&FormatView> {
        self.ui.format.view.as_ref().filter(|view| !view.looks.is_empty())
    }

    /// How far the toggle or alignment `press` is on for the selection.
    pub fn press_state(&self, press: Press) -> Pressed {
        // Auto Size is the page's and not the selection's.
        if press == Press::AutoSize {
            return if self.auto_size_on() { Pressed::On } else { Pressed::Off };
        }
        // With a text box open the toggles and the alignment show its characters (`shape.formatting` with a range), not the shape's first run.
        if let Some(state) = self.ui.format.range.as_ref().and_then(|(_, range)| range_pressed(range, press)) {
            return state;
        }
        self.press_state_of_view(press)
    }

    /// How far `press` is on for the shapes selected, from the state of each shape's first run (not the range of a text box).
    fn press_state_of_view(&self, press: Press) -> Pressed {
        let Some(view) = self.format_view() else { return Pressed::Off };
        match press {
            Press::None | Press::AutoSize => Pressed::Off,
            Press::Bold => view.pressed(|l| l.bold),
            Press::Italic => view.pressed(|l| l.italic),
            Press::Underline => view.pressed(|l| l.underline),
            Press::Strike => view.pressed(|l| l.strike),
            Press::Left => view.pressed(|l| l.align == Align::Left),
            Press::Center => view.pressed(|l| l.align == Align::Center),
            Press::Right => view.pressed(|l| l.align == Align::Right),
            Press::Justify => view.pressed(|l| l.align == Align::Justify),
            Press::Distributed => view.pressed(|l| l.align == Align::Distributed),
            Press::Top => view.pressed(|l| l.valign == VAlign::Top),
            Press::Middle => view.pressed(|l| l.valign == VAlign::Middle),
            Press::Bottom => view.pressed(|l| l.valign == VAlign::Bottom),
            Press::Bullets => view.pressed(|l| l.bullet != 0),
            Press::Vertical => view.pressed(|l| l.vertical),
            Press::Ltr => view.pressed(|l| l.direction == Direction::LeftToRight),
            Press::Rtl => view.pressed(|l| l.direction == Direction::RightToLeft),
            Press::AllCaps => view.pressed(|l| l.case == Case::AllCaps),
            Press::SmallCaps => view.pressed(|l| l.case == Case::SmallCaps),
        }
    }

    /// The case the letters of the selection show: the characters selected in the text box that is open, else each shape's first run.
    pub fn case_mix(&self) -> Mix<Case> {
        if let Some((_, range)) = &self.ui.format.range {
            return if range.mixed.case { Mix::Mixed } else { Mix::One(range.look.case) };
        }
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.case))
    }

    /// The bullet of the paragraphs of the selection (0 for none).
    pub fn bullet_mix(&self) -> Mix<u8> {
        if let Some((_, range)) = &self.ui.format.range {
            return if range.mixed.bullet { Mix::Mixed } else { Mix::One(range.look.bullet) };
        }
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.bullet))
    }

    /// The fill colours of the selection: `One(None)` is a fill that paints nothing.
    pub fn fill_mix(&self) -> Mix<Option<Rgb>> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fill))
    }

    /// The line colours of the selection.
    pub fn line_mix(&self) -> Mix<Option<Rgb>> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.line))
    }

    /// The text colours of the selection.
    pub fn text_colour_mix(&self) -> Mix<Rgb> {
        if let Some((_, range)) = &self.ui.format.range {
            return if range.mixed.color { Mix::Mixed } else { Mix::One(range.look.color) };
        }
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.color))
    }

    /// The face the selection is in, if every shape has one and it is the same.
    pub fn font_mix(&self) -> Mix<Option<String>> {
        if let Some((_, range)) = &self.ui.format.range {
            return if range.mixed.font { Mix::Mixed } else { Mix::One(range.look.font.clone()) };
        }
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.font.clone()))
    }

    /// The size of the text of the selection, in thousandths of a point.
    pub fn size_mix(&self) -> Mix<i64> {
        if let Some((_, range)) = &self.ui.format.range {
            return if range.mixed.size { Mix::Mixed } else { Mix::One(thousandths(range.look.size)) };
        }
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| thousandths(l.size)))
    }

    /// The weight of the lines of the selection, in thousandths of a point.
    pub fn weight_mix(&self) -> Mix<i64> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| thousandths(l.weight)))
    }

    /// The dash pattern, begin arrow and end arrow numbers of the selection.
    pub fn pattern_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.pattern))
    }

    pub fn begin_arrow_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.begin_arrow))
    }

    pub fn end_arrow_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.end_arrow))
    }

    /// The size class of the line end at the start (`BeginArrowSize`, 0 to 6) the shapes selected agree on.
    pub fn begin_arrow_size_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.begin_arrow_size))
    }

    /// The size class of the line end at the end (`EndArrowSize`, 0 to 6) the shapes selected agree on.
    pub fn end_arrow_size_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.end_arrow_size))
    }

    // ---------------------------------------------------------------------------------------------------- the commands

    /// The shapes selected as the page on view has them: **the selection counts only when it was made on the page on view** (for the commands, 74 for
    /// the panes, the review). A shape's `ID` is the page's own, so a page that holds shapes 1 and 2 as well as the page the selection was made on
    /// has its shapes formatted, turned, cut or united by a command that took the page on view, and a pane that shows the page on view's shape 1 for a selection
    /// of another page's takes a value typed in it for the wrong shape: `edit.select {ids: [1, 2]}`, `page.select {id: 2}` and a command, served in one frame by
    /// the control channel, did the first, and a pane drawn in that frame shows the second page's values. A person cannot reach it (the next frame drops a
    /// selection that is not of the page on view, `ensure_scene`), so this is for scripts, and it is a *refusal* and not a drop of the selection here: a command
    /// and a pane must not change what the window holds. **Everything that pairs a page with the selection asks here** ([`DiacraftApp::selection_target`] for the
    /// commands, [`DiacraftApp::selection_here`] for the panes, the formatting the controls show), so that there is one rule.
    pub(crate) fn selection_on_view(&mut self) -> OnView {
        let Some(shown) = self.current_page() else { return OnView::NoDocument };
        let ids = self.ui.edit.selection().to_vec();
        if ids.is_empty() {
            return OnView::Nothing;
        }
        match self.ui.edit.selection_page() {
            Some(page) if page == shown => OnView::Here(page, ids),
            _ => OnView::Elsewhere,
        }
    }

    /// The page on view and the shapes selected on it, for a pane or a control that shows or edits them; `None` with nothing selected, and **with a selection
    /// that is of another page** (nothing editable and nothing of the other page's is shown).
    pub(crate) fn selection_here(&mut self) -> Option<(u32, Vec<u32>)> {
        match self.selection_on_view() {
            OnView::Here(page, ids) => Some((page, ids)),
            _ => None,
        }
    }

    /// The page the shapes selected are on and the shapes, for a command that acts on them: the selection's own page, and a refusal in words when that is not the
    /// page on view (see [`DiacraftApp::selection_on_view`]).
    pub(crate) fn selection_target(&mut self, cmd: &str) -> Result<(u32, Vec<u32>)> {
        let refused = |msg: &str| EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.to_owned() };
        match self.selection_on_view() {
            OnView::NoDocument => Err(refused("no document open")),
            OnView::Nothing => Err(refused("nothing is selected")),
            OnView::Elsewhere => Err(refused(SELECTION_ELSEWHERE)),
            OnView::Here(page, ids) => Ok((page, ids)),
        }
    }

    /// `format.shape`: `shape.format` of the selection.
    pub(crate) fn format_shape(&mut self, p: &Map<String, Value>) -> Result<()> {
        let (page, ids) = self.selection_target("format.shape")?;
        let mut params = p.clone();
        params.insert("ids".to_owned(), json!(ids));
        params.insert("page".to_owned(), json!(page));
        // A label being typed is sent first, and the box opens again after, with the same characters selected (as `format_text` does).
        let open = self.open_text_selection();
        self.commit_pending_text();
        let done = self.execute("shape.format", &Value::Object(params)).map(|_| ());
        if let Some(open) = open {
            self.reopen_text(open.page, open.id, open.range);
        }
        done
    }

    /// `format.text`: `text.format` of the selection, or of the characters selected in the text box that is open.
    pub(crate) fn format_text(&mut self, p: &Map<String, Value>) -> Result<()> {
        self.format_text_of(p, None)
    }

    /// `format.text`, and for a toggle (`Some((what, press))`) the value to send is worked out **after what was typed is sent and from what the
    /// controls show**, the characters selected in the open box or the run before its caret: off when they all have the style, on otherwise, a
    /// mixed range included; with no box open, from the state of each shape's first run as `shape.formatting` gives it.
    fn format_text_of(&mut self, p: &Map<String, Value>, toggle: Option<(&str, Press)>) -> Result<()> {
        const CMD: &str = "format.text";
        // The box first: its range is read before sending what was typed closes it.
        let open = self.open_text_selection();
        // `shown`: the range the controls were showing, read again from the drawing once what was typed is in it (a caret is a point).
        let (page, ids, range, shown) = match open {
            Some(open) => {
                self.commit_pending_text();
                let within = open.range.filter(|(_, to)| self.text_chars(open.page, open.id).is_some_and(|n| *to <= n));
                (open.page, vec![open.id], within.filter(|(from, to)| from < to), within)
            }
            None => {
                let (page, ids) = self.selection_target(CMD)?;
                (page, ids, None, None)
            }
        };
        let mut params = p.clone();
        if let Some((what, press)) = toggle {
            let read = shown.zip(ids.first()).and_then(|((from, to), id)| self.read_range(page, *id, from, to));
            let now = match read.as_ref().and_then(|look| range_pressed(look, press)) {
                Some(state) => state,
                None => {
                    self.refresh_format_view();
                    self.press_state_of_view(press)
                }
            };
            let (key, value) = toggle_param(what, now != Pressed::On);
            params.insert(key.to_owned(), value);
        }
        params.insert("ids".to_owned(), json!(ids));
        params.insert("page".to_owned(), json!(page));
        if let Some((start, end)) = range {
            params.insert("start".to_owned(), json!(start));
            params.insert("end".to_owned(), json!(end));
        }
        let done = self.execute("text.format", &Value::Object(params)).map(|_| ());
        if let Some(open) = open {
            // The person goes on typing, with the same characters selected.
            self.reopen_text(open.page, open.id, open.range);
        }
        done
    }

    /// `format.toggle`: Bold, Italic, Underline, Strikethrough, Bullets, Vertical Text, All Caps or Small Caps on the selection, off when every shape has it
    /// and on otherwise.
    pub(crate) fn format_toggle(&mut self, what: &str) -> Result<()> {
        const CMD: &str = "format.toggle";
        let press = match what {
            "bold" => Press::Bold,
            "italic" => Press::Italic,
            "underline" => Press::Underline,
            "strike" => Press::Strike,
            "bullets" => Press::Bullets,
            "vertical" => Press::Vertical,
            "all_caps" => Press::AllCaps,
            "small_caps" => Press::SmallCaps,
            other => {
                return Err(EngineError::BadParams {
                    cmd: CMD.to_owned(),
                    msg: format!(
                        "`what` must be bold, italic, underline, strike, bullets, vertical, all_caps or small_caps, not `{}`",
                        shown(other, 32)
                    ),
                });
            }
        };
        self.format_text_of(&Map::new(), Some((what, press)))
    }

    /// `format.size`: Increase or Decrease Font Size.
    pub(crate) fn format_size_step(&mut self, step: &str) -> Result<()> {
        const CMD: &str = "format.size";
        let up = match step {
            "up" => true,
            "down" => false,
            other => {
                return Err(EngineError::BadParams { cmd: CMD.to_owned(), msg: format!("`step` must be up or down, not `{}`", shown(other, 32)) });
            }
        };
        self.refresh_format_view();
        // With a box open the size is that of the characters it shows (their size when they agree, else the first run's, which is what the engine gives).
        let first = match &self.ui.format.range {
            Some((_, range)) => range.look.size,
            None => self.format_view().and_then(|v| v.looks.first()).map_or(12.0, |l| l.size),
        };
        let mut params = Map::new();
        params.insert("size".to_owned(), json!(step_size(first, up)));
        self.format_text(&params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_goes_to_the_next_size_of_the_list() {
        assert_eq!(step_size(12.0, true), 14.0);
        assert_eq!(step_size(12.0, false), 11.0);
        assert_eq!(step_size(11.5, true), 12.0);
        assert_eq!(step_size(11.5, false), 11.0);
        assert_eq!(step_size(72.0, true), 84.0);
        assert_eq!(step_size(6.0, false), 5.0);
        assert_eq!(step_size(1.0, false), 1.0);
        assert_eq!(step_size(999.0, true), 1000.0);
        assert_eq!(step_size(f64::NAN, true), 14.0);
        // The size a file holds as inches times 72 is a little off a whole number.
        assert_eq!(step_size(11.999999999999998, true), 14.0);
    }

    #[test]
    fn a_mixed_selection_shows_nothing_and_a_toggle_is_half_pressed() {
        let look = |id: u32, bold: bool| ShapeLook {
            id,
            fill: None,
            line: None,
            weight: 0.75,
            pattern: 1,
            begin_arrow: 0,
            end_arrow: 0,
            begin_arrow_size: 2,
            end_arrow_size: 2,
            rounding: 0.0,
            font: None,
            size: 12.0,
            bold,
            italic: false,
            underline: false,
            strike: false,
            color: Rgb(0, 0, 0),
            align: Align::Center,
            valign: VAlign::Middle,
            bullet: 0,
            ind_left: 0.0,
            case: Case::Normal,
            direction: Direction::LeftToRight,
            vertical: false,
            fx: diacraft_engine::formatting::FxLook {
                fill_pattern: 0,
                fill_transparency: 0.0,
                fill_background: None,
                line_transparency: 0.0,
                line_cap: 0,
                compound: 0,
                gradient: None,
                line_gradient: None,
                shadow: None,
            },
        };
        let view = |bolds: &[bool]| FormatView {
            key: (0, 0, 1, Vec::new(), true),
            looks: bolds.iter().enumerate().map(|(i, b)| look(u32::try_from(i).unwrap_or(0), *b)).collect(),
        };
        assert_eq!(view(&[]).mix(|l| l.bold), Mix::Nothing);
        assert_eq!(view(&[true, true]).mix(|l| l.bold), Mix::One(true));
        assert_eq!(view(&[true, false]).mix(|l| l.bold), Mix::Mixed);
        assert_eq!(view(&[true, true]).pressed(|l| l.bold), Pressed::On);
        assert_eq!(view(&[true, false]).pressed(|l| l.bold), Pressed::Part);
        assert_eq!(view(&[false, false]).pressed(|l| l.bold), Pressed::Off);
    }
}
