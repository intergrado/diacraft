//! Transparency, gradients, patterns, shadows and Quick Styles in the window: what the controls read of the selection, and the window's commands
//! `format.gradient`, `format.shadow` and `format.quickStyle`.
//!
//! **What the controls show** is what `shape.formatting` says of each shape selected (`FxLook`, in the window's `FormatView`), under the rule: a control shows
//! a value when every shape has it, and nothing when they differ. **The window commands** are the engine's `shape.gradient`, `shape.shadow` and `shape.quick_style` with
//! the shapes selected on the page on view, as `format.shape` is `shape.format` (a label being typed is sent first, and the box opens again after). `format.gradient` also
//! takes the edits of one stop, which only the window knows how to make because it knows the stops the shape has now: `stop_color`, `stop_position` (0 to 1) and
//! `stop_transparency` (percent) of the stop `stop` (the one the pane has selected when it is left out), `add_stop` (a stop in the widest gap between two, in the colour
//! between theirs) and `remove_stop`. The window sends the whole list of stops to the engine, which is the one place a gradient is written.
//!
//! **A slider that is dragged shows its value on the page as it goes and is one undo step** (`live_send`): each frame of the drag takes back the step the last frame made
//! and makes the step again with the new value, so the picture is the live preview the Format Shape pane has in the reference and one Undo
//! takes the whole drag back. It does that only while nothing else has changed the drawing since its own last step (the revision is the one it left).

use diacraft_engine::formatting::{GradientLook, GradientTarget, QuickLook, ShadowLook, StopLook};
use diacraft_engine::{EngineError, Result};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::format::Mix;

/// What kind of fill or line a shape has, as the radio buttons of the Format Shape pane list them: the line has no pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintKind {
    None,
    Solid,
    Gradient,
    Pattern,
}

/// What the Format Shape pane remembers for its transparency, gradient and pattern controls.
#[derive(Debug)]
pub struct FxState {
    /// The stop of the gradient the pane edits (an index of the stops in position order).
    pub stop: usize,
    /// The slider that is being dragged, and the drawing's revision after the step its last frame made (`live_send`).
    pub live: Option<(String, u64)>,
    /// The pattern a Pattern fill starts as, and the colour between its marks: the last the pane gave, or the ones this starts with.
    pub pattern: u8,
    pub background: String,
}

impl Default for FxState {
    fn default() -> Self {
        // **Ours** (no specification says what Pattern fill starts as): the first hatch, over white.
        FxState { stop: 0, live: None, pattern: 2, background: "#ffffff".to_owned() }
    }
}

/// What the Quick Styles gallery shows, read for the first shape selected and kept for the revision, the page and the shape it was read for.
#[derive(Debug, Default)]
pub struct QuickState {
    key: Option<(u64, u64, u32, u32)>,
    pub look: Option<QuickLook>,
    /// Why there are no tiles: the engine's words (the page has no theme to take a style from, nothing is selected).
    pub why: Option<String>,
}

/// The most stops a gradient has (the engine's own cap).
const MAX_STOPS: usize = diacraft_engine::formatting::MAX_GRADIENT_STOPS;

impl DiacraftApp {
    /// The kind of fill the shapes selected have.
    pub(crate) fn fill_kind_mix(&self) -> Mix<PaintKind> {
        self.format_view().map_or(Mix::Nothing, |v| {
            v.mix(|l| match (l.fill, l.fx.fill_pattern) {
                (None, _) => PaintKind::None,
                (Some(_), 25..=40) => PaintKind::Gradient,
                _ if l.fx.gradient.is_some() => PaintKind::Gradient,
                (Some(_), 2..=24) => PaintKind::Pattern,
                _ => PaintKind::Solid,
            })
        })
    }

    /// The kind of line the shapes selected have.
    pub(crate) fn line_kind_mix(&self) -> Mix<PaintKind> {
        self.format_view().map_or(Mix::Nothing, |v| {
            v.mix(|l| match (l.line, &l.fx.line_gradient) {
                (None, _) => PaintKind::None,
                (Some(_), Some(_)) => PaintKind::Gradient,
                _ => PaintKind::Solid,
            })
        })
    }

    /// A percentage of the selection in hundredths of a percent, for [`Mix`]: the fill's or the line's transparency.
    pub(crate) fn fill_transparency_mix(&self) -> Mix<i64> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| hundredths(l.fx.fill_transparency)))
    }

    pub(crate) fn line_transparency_mix(&self) -> Mix<i64> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| hundredths(l.fx.line_transparency)))
    }

    /// The pattern number of the fill (`FillPattern`) and the colour between its marks.
    pub(crate) fn pattern_fill_mix(&self) -> Mix<u32> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fx.fill_pattern))
    }

    pub(crate) fn background_mix(&self) -> Mix<Option<diacraft_engine::formatting::Rgb>> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fx.fill_background))
    }

    /// The cap and the compound line of the lines of the selection (the cells' numbers).
    pub(crate) fn cap_mix(&self) -> Mix<u8> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fx.line_cap))
    }

    pub(crate) fn compound_mix(&self) -> Mix<u8> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fx.compound))
    }

    /// The gradient of the fill or the line of the selection: one when the shapes agree (or have none), mixed when they differ.
    pub(crate) fn gradient_mix(&self, target: GradientTarget) -> Mix<Option<GradientLook>> {
        self.format_view().map_or(Mix::Nothing, |v| {
            v.mix(|l| match target {
                GradientTarget::Fill => l.fx.gradient.clone(),
                GradientTarget::Line => l.fx.line_gradient.clone(),
            })
        })
    }

    /// The shadow of the selection: one when the shapes agree (or have none), mixed when they differ.
    pub(crate) fn shadow_mix(&self) -> Mix<Option<ShadowLook>> {
        self.format_view().map_or(Mix::Nothing, |v| v.mix(|l| l.fx.shadow.clone()))
    }

    /// The stop of the gradient the pane edits, clamped to the stops there are.
    pub(crate) fn selected_stop(&self, stops: usize) -> usize {
        self.ui.panes.format.fx.stop.min(stops.saturating_sub(1))
    }

    // ---------------------------------------------------------------------------------------------------- the commands

    /// `format.gradient`: `shape.gradient` of the selection, and the edits of one stop (the module documentation).
    pub(crate) fn format_gradient(&mut self, p: &Map<String, Value>) -> Result<()> {
        const CMD: &str = "format.gradient";
        let bad = |msg: &str| EngineError::BadParams { cmd: CMD.to_owned(), msg: msg.to_owned() };
        let mut params = p.clone();
        let edits = ["stop_color", "stop_position", "stop_transparency", "add_stop", "remove_stop"];
        if edits.iter().any(|k| params.get(*k).is_some_and(|v| !v.is_null())) {
            let target = match params.get("target").and_then(Value::as_str) {
                None => GradientTarget::Fill,
                Some(word) => GradientTarget::from_word(word).ok_or_else(|| bad("`target` must be fill or line"))?,
            };
            self.refresh_format_view();
            let held = self.format_view().and_then(|v| v.looks.first()).and_then(|l| match target {
                GradientTarget::Fill => l.fx.gradient.clone(),
                GradientTarget::Line => l.fx.line_gradient.clone(),
            });
            let Some(held) = held else { return Err(bad("the shape has no gradient: turn Gradient fill on first")) };
            let at = match params.get("stop") {
                None | Some(Value::Null) => self.selected_stop(held.stops.len()),
                Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("`stop` must be a whole number"))?,
            };
            let (stops, selected) = edit_stops(&held.stops, at, &params).map_err(|msg| bad(&msg))?;
            params.insert("stops".to_owned(), stops_json(&stops));
            for key in edits.iter().chain(["stop"].iter()) {
                params.remove(*key);
            }
            self.ui.panes.format.fx.stop = selected;
        }
        self.selection_command(CMD, "shape.gradient", params)
    }

    /// `format.shadow`: `shape.shadow` of the selection; `shadow_color` is the engine's `color` (the palette of the pane names the colour it sends by what it is for).
    pub(crate) fn format_shadow(&mut self, p: &Map<String, Value>) -> Result<()> {
        let mut params = p.clone();
        if let Some(colour) = params.remove("shadow_color") {
            params.insert("color".to_owned(), colour);
        }
        self.selection_command("format.shadow", "shape.shadow", params)
    }

    /// `format.quickStyle`: `shape.quick_style` of the selection.
    pub(crate) fn format_quick_style(&mut self, p: &Map<String, Value>) -> Result<()> {
        self.selection_command("format.quickStyle", "shape.quick_style", p.clone())
    }

    /// The engine command `engine` with `params` and the shapes selected on the page on view, as `format.shape` sends `shape.format`: what was typed in a label is sent first
    /// and its box opens again after.
    fn selection_command(&mut self, window_id: &str, engine: &str, mut params: Map<String, Value>) -> Result<()> {
        let (page, ids) = self.selection_target(window_id)?;
        params.insert("ids".to_owned(), json!(ids));
        params.insert("page".to_owned(), json!(page));
        let open = self.open_text_selection();
        self.commit_pending_text();
        let done = self.execute(engine, &Value::Object(params)).map(|_| ());
        if let Some(open) = open {
            self.reopen_text(open.page, open.id, open.range);
        }
        done
    }

    /// Sends `command` with `params` for a slider that is being dragged: the first frame of the drag makes a step, and each frame after takes that step back and makes it again
    /// with the new value, while the drawing is as the last frame left it (one undo step for the whole drag, and the page showing the value as it goes).
    pub(crate) fn live_send(&mut self, key: &str, command: &str, params: &Value) {
        let revision = self.session.revision();
        let again = self.ui.panes.format.fx.live.as_ref().is_some_and(|(held, rev)| held == key && *rev == revision);
        if again && let Err(e) = self.execute("edit.undo", &json!({})) {
            self.set_notice(&e.to_string());
            self.ui.panes.format.fx.live = None;
            return;
        }
        match self.execute(command, params) {
            Ok(_) => self.ui.panes.format.fx.live = Some((key.to_owned(), self.session.revision())),
            Err(e) => {
                self.set_notice(&e.to_string());
                self.ui.panes.format.fx.live = None;
            }
        }
    }

    /// The drag of a slider ended: its next change is a step of its own.
    pub(crate) fn live_end(&mut self) {
        self.ui.panes.format.fx.live = None;
    }

    // ---------------------------------------------------------------------------------------------------- the gallery

    /// Reads the Quick Styles gallery for the first shape selected, if the drawing, the page or the shape has changed since it was read. Called when the gallery's menu draws.
    pub(crate) fn refresh_quick_view(&mut self) {
        let Some((page, ids)) = self.selection_here() else {
            self.ui.quick = QuickState::default();
            return;
        };
        let first = ids.first().copied().unwrap_or(0);
        let key = (self.ui.generation, self.session.revision(), page, first);
        if self.ui.quick.key == Some(key) {
            return;
        }
        let read = self.session.document().map(|doc| diacraft_engine::formatting::quick_tiles(doc, page, &[first]));
        self.ui.quick = match read {
            Some(Ok(look)) => QuickState { key: Some(key), look: Some(look), why: None },
            Some(Err(problem)) => QuickState { key: Some(key), look: None, why: Some(problem.message) },
            None => QuickState::default(),
        };
    }

    /// Why Quick Styles cannot be used now, for a quiet control's tooltip: nothing is selected, or the page has no theme.
    pub(crate) fn quick_refusal(&self) -> Option<String> {
        let Some(doc) = self.session.document() else { return Some("no document open".to_owned()) };
        let page = self.page_on_view()?;
        let themed = diacraft_engine::theme::selection_of(doc, page).is_some_and(|s| s.is_themed());
        (!themed).then(|| "the page has no theme: apply one first (Design, Themes)".to_owned())
    }
}

/// The opacity (0 to 255) of a colour that is `transparency` percent clear.
pub(crate) fn alpha_of(transparency: f64) -> u8 {
    if transparency.is_finite() { ((100.0 - transparency).clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8 } else { 255 }
}

/// The colour and opacity (0 to 255) of a gradient at `at` (0 to 1): linear between the stops, the nearest one beyond the ends.
pub(crate) fn gradient_colour_at(g: &GradientLook, at: f64) -> (u8, u8, u8, u8) {
    let opaque = |s: &StopLook| (s.colour.0, s.colour.1, s.colour.2, alpha_of(s.transparency));
    let Some(first) = g.stops.first() else { return (0, 0, 0, 0) };
    if at <= first.position {
        return opaque(first);
    }
    for pair in g.stops.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if at <= b.position {
            let span = (b.position - a.position).max(1.0e-9);
            let k = ((at - a.position) / span).clamp(0.0, 1.0);
            let (ca, cb) = (opaque(a), opaque(b));
            let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * k).round() as u8;
            return (mix(ca.0, cb.0), mix(ca.1, cb.1), mix(ca.2, cb.2), mix(ca.3, cb.3));
        }
    }
    g.stops.last().map_or((0, 0, 0, 0), opaque)
}

/// A percentage in hundredths of a percent, as a whole number a [`Mix`] can compare.
pub(crate) fn hundredths(percent: f64) -> i64 {
    if percent.is_finite() { (percent * 100.0).round() as i64 } else { 0 }
}

/// The stops as the engine's `stops` parameter takes them.
fn stops_json(stops: &[StopLook]) -> Value {
    Value::Array(stops.iter().map(|s| json!({"position": s.position, "color": s.colour.hex(), "transparency": s.transparency})).collect())
}

/// The stops after the edit `params` name on the stop `at`, and the place the edited stop has in them (they are in position order). A refusal is in words.
fn edit_stops(held: &[StopLook], at: usize, params: &Map<String, Value>) -> std::result::Result<(Vec<StopLook>, usize), String> {
    let mut list: Vec<(StopLook, bool)> = held.iter().map(|s| (*s, false)).collect();
    let at = at.min(list.len().saturating_sub(1));
    let number = |key: &str, from: f64, to: f64| -> std::result::Result<Option<f64>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                v.as_f64().filter(|n| (from..=to).contains(n)).map(Some).ok_or_else(|| format!("`{key}` must be a number from {from} to {to}"))
            }
        }
    };
    if params.get("remove_stop").and_then(Value::as_bool) == Some(true) {
        if list.len() <= 2 {
            return Err("a gradient has at least 2 stops".to_owned());
        }
        list.remove(at);
        let next = at.min(list.len() - 1);
        if let Some(entry) = list.get_mut(next) {
            entry.1 = true;
        }
    } else if params.get("add_stop").and_then(Value::as_bool) == Some(true) {
        if list.len() >= MAX_STOPS {
            return Err(format!("a gradient has at most {MAX_STOPS} stops"));
        }
        // In the widest gap between two stops (or between an end and the stop nearest it), in the colour between the two stops that bound it.
        let mut best = (0.0_f64, 0.0_f64, 0.0_f64, held.first().copied(), held.first().copied());
        let ends: Vec<(f64, Option<StopLook>)> =
            std::iter::once((0.0, None)).chain(held.iter().map(|s| (s.position, Some(*s)))).chain(std::iter::once((1.0, None))).collect();
        for pair in ends.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b.0 - a.0 > best.0 {
                best = (b.0 - a.0, a.0, b.0, a.1.or(b.1), b.1.or(a.1));
            }
        }
        let (_, from, to, left, right) = best;
        let (left, right) = (left.unwrap_or(held[0]), right.unwrap_or(held[0]));
        let mix = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
        let colour = diacraft_engine::formatting::Rgb(
            mix(left.colour.0, right.colour.0),
            mix(left.colour.1, right.colour.1),
            mix(left.colour.2, right.colour.2),
        );
        let transparency = (left.transparency + right.transparency) / 2.0;
        list.push((StopLook { position: ((from + to) / 2.0 * 1000.0).round() / 1000.0, colour, transparency }, true));
    } else if let Some(entry) = list.get_mut(at) {
        if let Some(hex) = params.get("stop_color").and_then(Value::as_str) {
            entry.0.colour = diacraft_engine::formatting::Rgb::parse(hex).ok_or_else(|| "`stop_color` must be a colour like #3C78D8".to_owned())?;
        }
        if let Some(position) = number("stop_position", 0.0, 1.0)? {
            entry.0.position = position;
        }
        if let Some(transparency) = number("stop_transparency", 0.0, 100.0)? {
            entry.0.transparency = transparency;
        }
        entry.1 = true;
    }
    list.sort_by(|a, b| a.0.position.total_cmp(&b.0.position));
    let selected = list.iter().position(|(_, mark)| *mark).unwrap_or(0);
    Ok((list.into_iter().map(|(s, _)| s).collect(), selected))
}

#[cfg(test)]
mod tests {
    use diacraft_engine::formatting::Rgb;
    use serde_json::json;

    use super::*;

    fn stop(position: f64, colour: Rgb) -> StopLook {
        StopLook { position, colour, transparency: 0.0 }
    }

    fn params(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn a_stop_added_goes_in_the_widest_gap_in_the_colour_between_its_neighbours_and_is_the_one_selected() {
        let held = [stop(0.0, Rgb(255, 0, 0)), stop(0.2, Rgb(0, 255, 0)), stop(1.0, Rgb(0, 0, 255))];
        let (list, selected) = edit_stops(&held, 0, &params(json!({"add_stop": true}))).unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!((list[2].position, list[2].colour), (0.6, Rgb(0, 127, 127)), "between the green at 0.2 and the blue at 1");
        assert_eq!(selected, 2);
    }

    #[test]
    fn a_stop_moved_past_another_is_still_the_one_selected() {
        let held = [stop(0.0, Rgb(255, 0, 0)), stop(0.5, Rgb(0, 255, 0)), stop(1.0, Rgb(0, 0, 255))];
        let (list, selected) = edit_stops(&held, 0, &params(json!({"stop_position": 0.75}))).unwrap();
        assert_eq!(list[0].colour, Rgb(0, 255, 0));
        assert_eq!((list[1].colour, selected), (Rgb(255, 0, 0), 1), "the red one moved past the green, and it is still the stop selected");
    }

    #[test]
    fn a_gradient_keeps_two_stops_and_at_most_ten() {
        let two = [stop(0.0, Rgb(0, 0, 0)), stop(1.0, Rgb(255, 255, 255))];
        assert!(edit_stops(&two, 0, &params(json!({"remove_stop": true}))).unwrap_err().contains("at least 2"));
        let ten: Vec<StopLook> = (0..10).map(|i| stop(f64::from(i) / 9.0, Rgb(0, 0, 0))).collect();
        assert!(edit_stops(&ten, 0, &params(json!({"add_stop": true}))).unwrap_err().contains("at most 10"));
        let three = [stop(0.0, Rgb(0, 0, 0)), stop(0.5, Rgb(9, 9, 9)), stop(1.0, Rgb(255, 255, 255))];
        let (list, selected) = edit_stops(&three, 2, &params(json!({"remove_stop": true}))).unwrap();
        assert_eq!((list.len(), selected), (2, 1), "the last stop went and the one before it is the one selected");
    }

    #[test]
    fn a_stop_edit_refuses_what_is_not_a_colour_or_a_place_in_words() {
        let one = [stop(0.0, Rgb(0, 0, 0)), stop(1.0, Rgb(255, 255, 255))];
        assert!(edit_stops(&one, 0, &params(json!({"stop_color": "red"}))).unwrap_err().contains("#3C78D8"));
        assert!(edit_stops(&one, 0, &params(json!({"stop_position": 1.5}))).unwrap_err().contains("from 0 to 1"));
        assert!(edit_stops(&one, 0, &params(json!({"stop_transparency": -1}))).unwrap_err().contains("0 to 100"));
    }

    #[test]
    fn hundredths_of_a_percent_are_whole_numbers_that_compare() {
        assert_eq!(hundredths(35.0), 3500);
        assert_eq!(hundredths(0.005 * 100.0), 50);
        assert_eq!(hundredths(f64::NAN), 0);
    }

    #[test]
    fn a_gradient_is_linear_between_its_stops_and_the_nearest_stop_beyond_them() {
        let g = GradientLook {
            dir: 0,
            angle: 90.0,
            rotate_with_shape: true,
            stops: vec![stop(0.25, Rgb(0, 0, 0)), StopLook { position: 0.75, colour: Rgb(200, 100, 0), transparency: 50.0 }],
        };
        assert_eq!(gradient_colour_at(&g, 0.0), (0, 0, 0, 255), "before the first stop it is the first");
        assert_eq!(gradient_colour_at(&g, 0.5), (100, 50, 0, 192), "half way between, opacity too");
        assert_eq!(gradient_colour_at(&g, 1.0), (200, 100, 0, 128), "after the last stop it is the last");
        assert_eq!(gradient_colour_at(&GradientLook { stops: Vec::new(), ..g }, 0.5), (0, 0, 0, 0));
    }
}
