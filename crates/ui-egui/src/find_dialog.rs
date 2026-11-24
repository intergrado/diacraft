//! The Find and Replace dialogs: Home > Editing > Find > Find... and Replace....
//!
//! **Non-modal.** Unlike the other dialogs of the window, a Find dialog does not shade the window or take its keys and pointer: it is a window that floats over the drawing area, which
//! can be moved, and the drawing under it can be clicked, scrolled and edited while it is open (Find Next needs the drawing to be seen as it moves from one match to the next).
//! It is closed by its close button, Escape in its text box, and `window.find {close: true}`. It is not in [`DiacraftApp::is_modal_open`].
//!
//! **What the specification gives**: the Find dialog has **Find what** (a box that keeps earlier searches), **Special** (a tab character, a manual line break, an optional
//! hyphen and any character as a wildcard), the group **Search in** with the scope **Selection**, **Current page**, **All pages** and what to search, **Shape text**, **Shape data**,
//! **Shape name** and **User-defined cell**, the group **Options** with **Match case** and **Find whole words only**, a **Found in** line, and **Find Next**. The Replace dialog has
//! **Find what** and **Replace with** (both with history), **Special**, the scope as **Search**, the same options, and **Replace**, **Replace All** and **Find Next**, and does not
//! list what to search: **it replaces shape text** (the engine's `edit.replace` can do the other three, which the dialog leaves to a program). **Match character width** and **Match Kana type**
//! are for Japanese text and appear only when the Regional option for Kana is on: the window has no such option, so they are left out.
//!
//! **Defaults** are not stated. Ours, the ones that lose nothing and surprise least: the scope is the current page, and the search is in shape text alone.
//!
//! **What the buttons do.** The dialog only asks the engine. Find Next is `edit.find` from the place of the match it is at (with a limit of one, and going round to the start when it reaches
//! the end), then the window shows the match's page (`page.select`) and selects its shape (`edit.select`), and moves the view to it when it is out of sight. Replace is `edit.replace` of the
//! match it is at, then Find Next from after the new text; with no match to be at it finds one first. Replace All is one `edit.replace`, and one undo step. `window.find` does what the
//! buttons do for a program (`action`).

use egui::{Context, Id, Key, RichText, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use diacraft_engine::{EngineError, Result, shown};

/// How many earlier searches a box keeps.
pub const HISTORY: usize = 12;

/// The most characters a box takes: the engine's pattern is at most 1,024.
const FIELD_CHARS: usize = 1_024;

/// The dialog's width inside its frame, and about how tall it is (the Replace dialog, the taller): for where it will first be drawn.
const DIALOG_W: f32 = 340.0;
const DIALOG_H: f32 = 340.0;

/// The characters the Special list puts into Find what, with the words it is drawn with (a tab, a manual line break, an optional hyphen and any character).
pub const SPECIALS: [(&str, &str); 4] = [("Tab character", "^t"), ("Manual line break", "^l"), ("Optional hyphen", "^-"), ("Any character", "^?")];

/// Which of the two dialogs it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Find,
    Replace,
}

impl Mode {
    /// The word `window.find` takes and the window state gives.
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Find => "find",
            Mode::Replace => "replace",
        }
    }

    /// The mode a word names.
    pub fn from_name(name: &str) -> Option<Mode> {
        [Mode::Find, Mode::Replace].into_iter().find(|m| m.name().eq_ignore_ascii_case(name))
    }

    const fn title(self) -> &'static str {
        match self {
            Mode::Find => "Find",
            Mode::Replace => "Replace",
        }
    }
}

/// Where the dialog looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Selection,
    Page,
    All,
}

impl Scope {
    const ALL: [Scope; 3] = [Scope::Selection, Scope::Page, Scope::All];

    /// The word the engine and `window.find` take.
    pub const fn name(self) -> &'static str {
        match self {
            Scope::Selection => "selection",
            Scope::Page => "page",
            Scope::All => "all",
        }
    }

    pub fn from_name(name: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name))
    }

    const fn caption(self) -> &'static str {
        match self {
            Scope::Selection => "Selection",
            Scope::Page => "Current page",
            Scope::All => "All pages",
        }
    }
}

/// What a button was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Replace,
    ReplaceAll,
}

impl Action {
    pub fn from_name(name: &str) -> Option<Action> {
        match name {
            "next" => Some(Action::Next),
            "replace" => Some(Action::Replace),
            "replace_all" => Some(Action::ReplaceAll),
            _ => None,
        }
    }
}

/// What the line under the options says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// It is a refusal or a failure, and is drawn as one.
    pub problem: bool,
}

/// A Find or Replace dialog while it is open.
#[derive(Clone, Debug)]
pub struct FindDialog {
    pub mode: Mode,
    pub what: String,
    pub with: String,
    pub scope: Scope,
    pub in_text: bool,
    pub in_data: bool,
    pub in_name: bool,
    pub in_user: bool,
    pub match_case: bool,
    pub whole_words: bool,
    /// The match the last Find Next stopped at, as `edit.find` listed it.
    pub current: Option<Value>,
    pub line: Line,
    /// The box that has the keyboard is asked for on the next frame.
    focus: bool,
}

impl FindDialog {
    fn new(mode: Mode) -> FindDialog {
        FindDialog {
            mode,
            what: String::new(),
            with: String::new(),
            scope: Scope::Page,
            in_text: true,
            in_data: false,
            in_name: false,
            in_user: false,
            match_case: false,
            whole_words: false,
            current: None,
            line: Line::default(),
            focus: true,
        }
    }

    /// Where to look, as the engine's `in`. The Replace dialog replaces shape text alone.
    fn places(&self) -> Vec<&'static str> {
        if self.mode == Mode::Replace {
            return vec!["text"];
        }
        [(self.in_text, "text"), (self.in_data, "shape_data"), (self.in_name, "shape_name"), (self.in_user, "user_cell")]
            .into_iter()
            .filter_map(|(on, word)| on.then_some(word))
            .collect()
    }
}

/// The dialog and what it keeps between openings.
#[derive(Default)]
pub struct FindState {
    pub dialog: Option<FindDialog>,
    /// The searches earlier, newest first.
    history: Vec<String>,
    /// The replacements earlier, newest first.
    with_history: Vec<String>,
}

fn remember(list: &mut Vec<String>, text: &str) {
    if text.is_empty() {
        return;
    }
    list.retain(|t| t != text);
    list.insert(0, text.to_owned());
    list.truncate(HISTORY);
}

impl DiacraftApp {
    /// The dialog's mode while it is open (`None`: it is closed). For the window state, tests and agents.
    pub fn find_dialog_mode(&self) -> Option<&'static str> {
        self.ui.find.dialog.as_ref().map(|d| d.mode.name())
    }

    /// What the dialog shows, for the window state: `null` when it is closed.
    pub(crate) fn find_state(&self) -> Value {
        let Some(d) = self.ui.find.dialog.as_ref() else { return Value::Null };
        json!({
            "mode": d.mode.name(),
            "what": d.what,
            "with": d.with,
            "scope": d.scope.name(),
            "in": d.places(),
            "matchCase": d.match_case,
            "wholeWords": d.whole_words,
            "current": d.current,
            "line": d.line.text,
            "problem": d.line.problem,
            "history": self.ui.find.history,
        })
    }

    /// Opens the dialog in `mode`: a dialog that is open changes to it, with what was typed kept, and the box for the text asks for the keyboard.
    pub(crate) fn open_find_dialog(&mut self, mode: Mode) {
        match self.ui.find.dialog.as_mut() {
            Some(d) => {
                d.mode = mode;
                d.focus = true;
            }
            None => self.ui.find.dialog = Some(FindDialog::new(mode)),
        }
    }

    pub(crate) fn close_find_dialog(&mut self) {
        self.ui.find.dialog = None;
    }

    /// Forgets the dialog and its current match: another drawing replaced the one they were about.
    pub(crate) fn forget_find(&mut self) {
        self.ui.find.dialog = None;
    }

    /// `window.find`: opens the dialog, sets what is given, closes it, and does what a button does.
    pub(crate) fn find_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        const ID: &str = "window.find";
        let bad = |msg: String| EngineError::BadParams { cmd: ID.to_owned(), msg };
        if matches!(p.get("close"), Some(Value::Bool(true))) {
            self.close_find_dialog();
            return Ok(());
        }
        let text_of = |key: &str| -> Result<Option<String>> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) if s.chars().count() <= FIELD_CHARS => Ok(Some(s.clone())),
                Some(Value::String(_)) => Err(bad(format!("`{key}` must be at most {FIELD_CHARS} characters"))),
                Some(_) => Err(bad(format!("`{key}` must be text"))),
            }
        };
        let flag_of = |key: &str| -> Result<Option<bool>> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Bool(b)) => Ok(Some(*b)),
                Some(_) => Err(bad(format!("`{key}` must be true or false"))),
            }
        };
        let mode = match text_of("mode")? {
            None => self.ui.find.dialog.as_ref().map_or(Mode::Find, |d| d.mode),
            Some(word) => Mode::from_name(&word).ok_or_else(|| bad("`mode` must be find or replace".to_owned()))?,
        };
        let scope = match text_of("scope")? {
            None => None,
            Some(word) => Some(Scope::from_name(&word).ok_or_else(|| bad("`scope` must be selection, page or all".to_owned()))?),
        };
        let places: Option<[bool; 4]> = match p.get("in") {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) => {
                let mut on = [false; 4];
                for item in items {
                    match item.as_str() {
                        Some("text") => on[0] = true,
                        Some("shape_data") => on[1] = true,
                        Some("shape_name") => on[2] = true,
                        Some("user_cell") => on[3] = true,
                        _ => return Err(bad("`in` holds text, shape_data, shape_name or user_cell".to_owned())),
                    }
                }
                Some(on)
            }
            Some(_) => return Err(bad("`in` must be an array of text, shape_data, shape_name or user_cell".to_owned())),
        };
        let action = match text_of("action")? {
            None => None,
            Some(word) => Some(Action::from_name(&word).ok_or_else(|| bad("`action` must be next, replace or replace_all".to_owned()))?),
        };
        let (what, with, case, whole) = (text_of("what")?, text_of("with")?, flag_of("match_case")?, flag_of("whole_words")?);
        self.open_find_dialog(mode);
        if let Some(d) = self.ui.find.dialog.as_mut() {
            d.mode = mode;
            if let Some(what) = what {
                d.what = what;
                d.current = None;
            }
            if let Some(with) = with {
                d.with = with;
            }
            if let Some(scope) = scope {
                d.scope = scope;
                d.current = None;
            }
            if let Some([text, data, name, user]) = places {
                (d.in_text, d.in_data, d.in_name, d.in_user) = (text, data, name, user);
                d.current = None;
            }
            if let Some(case) = case {
                d.match_case = case;
                d.current = None;
            }
            if let Some(whole) = whole {
                d.whole_words = whole;
                d.current = None;
            }
        }
        match action {
            Some(Action::Next) => self.find_next(),
            Some(Action::Replace) => self.replace_current(),
            Some(Action::ReplaceAll) => self.replace_all(),
            None => {}
        }
        Ok(())
    }

    /// The params that say where and what to search, from the dialog's boxes: `(params, why not)`.
    fn find_params(&self) -> std::result::Result<Map<String, Value>, String> {
        let d = self.ui.find.dialog.as_ref().ok_or_else(|| "the dialog is not open".to_owned())?;
        if d.what.is_empty() {
            return Err("Type what to find.".to_owned());
        }
        let places = d.places();
        if places.is_empty() {
            return Err("Tick Shape text, Shape data, Shape name or User-defined cell to search in.".to_owned());
        }
        let mut params = Map::new();
        params.insert("what".to_owned(), json!(d.what));
        params.insert("match_case".to_owned(), json!(d.match_case));
        params.insert("whole_words".to_owned(), json!(d.whole_words));
        params.insert("in".to_owned(), json!(places));
        params.insert("scope".to_owned(), json!(d.scope.name()));
        match d.scope {
            Scope::All => {}
            Scope::Page => {
                let page = self.ui.page.ok_or_else(|| "There is no page to search.".to_owned())?;
                params.insert("page".to_owned(), json!(page));
            }
            Scope::Selection => {
                let ids = self.ui.edit.selection();
                if ids.is_empty() {
                    return Err("Nothing is selected: choose Current page or All pages, or select shapes.".to_owned());
                }
                let page = self.ui.edit.selection_page().or(self.ui.page).ok_or_else(|| "There is no page to search.".to_owned())?;
                params.insert("page".to_owned(), json!(page));
                params.insert("ids".to_owned(), json!(ids));
            }
        }
        Ok(params)
    }

    fn set_line(&mut self, text: String, problem: bool) {
        if let Some(d) = self.ui.find.dialog.as_mut() {
            d.line = Line { text, problem };
        }
    }

    /// The words of the Found in line for a match.
    fn found_in(&self, m: &Value, wrapped: bool) -> String {
        let name = m.get("shape_name").and_then(Value::as_str).map(|n| format!("“{}”", shown(n, 40)));
        let shape = name.unwrap_or_else(|| format!("shape {}", m.get("id").and_then(Value::as_u64).unwrap_or(0)));
        let row = m.get("row").and_then(Value::as_str).map(|r| shown(r, 40)).unwrap_or_default();
        let page = m
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok())
            .and_then(|id| self.session().document().and_then(|d| d.page(id).map(|p| shown(&p.name, 40))))
            .map(|n| format!(" on page “{n}”"))
            .unwrap_or_default();
        let place = match m.get("place").and_then(Value::as_str) {
            Some("shape_name") => format!("shape name of {shape}"),
            Some("shape_data") => format!("data row “{row}” of {shape}"),
            Some("user_cell") => format!("user-defined cell “{row}” of {shape}"),
            _ => format!("text block of {shape}"),
        };
        let round = if wrapped { " The search went round to the start." } else { "" };
        format!("Found in: {place}{page}.{round}")
    }

    /// Selects the shape of a match and shows its page, moving the view to the shape when it is out of sight.
    fn show_match(&mut self, m: &Value) -> Result<()> {
        let page = m
            .get("page")
            .and_then(Value::as_u64)
            .ok_or_else(|| EngineError::Failed { cmd: "window.find".to_owned(), msg: "a match has no page".to_owned() })?;
        let id = m
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| EngineError::Failed { cmd: "window.find".to_owned(), msg: "a match has no shape".to_owned() })?;
        if self.ui.page != u32::try_from(page).ok() {
            self.execute("page.select", &json!({ "id": page }))?;
        }
        self.execute("edit.select", &json!({ "ids": [id] }))?;
        self.reveal_selection();
        Ok(())
    }

    /// Pans the view so that the shape selected is in it and not under the dialog: a shape in view and clear of the dialog is left where it is, any other is brought to the
    /// middle of the largest part of the drawing area the dialog leaves free (the whole area when the dialog is closed).
    fn reveal_selection(&mut self) {
        let Some(id) = self.ui.edit.selection().first().copied() else { return };
        let Some(corners) = self.scene_corners(id) else { return };
        // Where the dialog is: where it was drawn, else (opened by the very command that is finding) where it will first be drawn.
        let dialog = self.ui.find.dialog.as_ref().map(|_| {
            let rect = self
                .element("find.window")
                .map_or_else(|| egui::Rect::from_min_size(self.find_default_pos(), vec2(DIALOG_W + 24.0, DIALOG_H)), |e| e.rect);
            rect.expand(8.0)
        });
        self.with_view(|view, area| {
            let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
            for (x, y) in corners {
                lo = (lo.0.min(x), lo.1.min(y));
                hi = (hi.0.max(x), hi.1.max(y));
            }
            let (a, b) = (view.to_screen(area, (lo.0, hi.1)), view.to_screen(area, (hi.0, lo.1)));
            let shape = egui::Rect::from_two_pos(a, b);
            if area.shrink(8.0).contains_rect(shape) && dialog.is_none_or(|d| !d.intersects(shape)) {
                return;
            }
            let mut free = vec![area];
            if let Some(d) = dialog {
                free = vec![
                    egui::Rect::from_min_max(area.min, pos2(d.min.x, area.max.y)),
                    egui::Rect::from_min_max(pos2(d.max.x, area.min.y), area.max),
                    egui::Rect::from_min_max(area.min, pos2(area.max.x, d.min.y)),
                    egui::Rect::from_min_max(pos2(area.min.x, d.max.y), area.max),
                ];
                free.retain(|r| r.width() >= shape.width() + 16.0 && r.height() >= shape.height() + 16.0);
                free.sort_by(|x, y| (y.width() * y.height()).total_cmp(&(x.width() * x.height())));
            }
            let target = free.first().map_or(area.center(), |r| r.center());
            view.pan_by(target - shape.center());
        });
    }

    /// Find Next: the first match after the one the dialog is at, going round.
    pub(crate) fn find_next(&mut self) {
        let mut params = match self.find_params() {
            Ok(p) => p,
            Err(words) => return self.set_line(words, true),
        };
        params.insert("limit".to_owned(), json!(1));
        let (what, current) = match self.ui.find.dialog.as_ref() {
            Some(d) => (d.what.clone(), d.current.clone()),
            None => return,
        };
        // From after the match it is at; a match that is no longer there (the text changed) is no place to go on from, and the search starts again at the start.
        if let Some(cur) = current.as_ref().and_then(Value::as_object) {
            let mut after = Map::new();
            for key in ["page", "id", "place", "row"] {
                if let Some(v) = cur.get(key) {
                    after.insert(key.to_owned(), v.clone());
                }
            }
            after.insert("offset".to_owned(), cur.get("end").cloned().unwrap_or(json!(0)));
            params.insert("after".to_owned(), Value::Object(after));
        }
        remember(&mut self.ui.find.history, &what);
        match self.execute("edit.find", &Value::Object(params)) {
            Err(e) => {
                self.set_line(e.to_string(), true);
                if let Some(d) = self.ui.find.dialog.as_mut() {
                    d.current = None;
                }
            }
            Ok(found) => match found.get("matches").and_then(Value::as_array).and_then(|m| m.first()).cloned() {
                None => {
                    self.set_line(format!("Can’t find “{}”.", shown(&what, 60)), true);
                    if let Some(d) = self.ui.find.dialog.as_mut() {
                        d.current = None;
                    }
                }
                Some(m) => {
                    let wrapped = found.get("wrapped").and_then(Value::as_bool).unwrap_or(false);
                    match self.show_match(&m) {
                        Ok(()) => {
                            let line = self.found_in(&m, wrapped);
                            self.set_line(line, false);
                            if let Some(d) = self.ui.find.dialog.as_mut() {
                                d.current = Some(m);
                            }
                        }
                        Err(e) => self.set_line(e.to_string(), true),
                    }
                }
            },
        }
    }

    /// Replace: the match the dialog is at is replaced, and Find Next goes on from after the new text. With no match to be at it finds one.
    pub(crate) fn replace_current(&mut self) {
        let Some((current, with)) = self.ui.find.dialog.as_ref().map(|d| (d.current.clone(), d.with.clone())) else { return };
        let Some(cur) = current.as_ref().and_then(Value::as_object).cloned() else { return self.find_next() };
        let mut params = match self.find_params() {
            Ok(p) => p,
            Err(words) => return self.set_line(words, true),
        };
        let mut at = Map::new();
        for key in ["page", "id", "place", "row", "start", "end"] {
            if let Some(v) = cur.get(key) {
                at.insert(key.to_owned(), v.clone());
            }
        }
        params.insert("at".to_owned(), Value::Object(at));
        params.insert("with".to_owned(), json!(with));
        // The one match is on one page: the scope the engine is asked about is that of the match.
        params.remove("scope");
        params.remove("ids");
        params.remove("page");
        remember(&mut self.ui.find.with_history, &with);
        match self.execute("edit.replace", &Value::Object(params)) {
            Err(e) => {
                self.set_line(e.to_string(), true);
                if let Some(d) = self.ui.find.dialog.as_mut() {
                    d.current = None;
                }
            }
            Ok(done) => {
                // Go on from after the new text: the match is the place the next search starts from.
                if let (Some(next), Some(d)) = (done.get("next").and_then(Value::as_object), self.ui.find.dialog.as_mut()) {
                    let mut at = Map::new();
                    for key in ["page", "id", "place", "row"] {
                        if let Some(v) = next.get(key) {
                            at.insert(key.to_owned(), v.clone());
                        }
                    }
                    at.insert("start".to_owned(), next.get("offset").cloned().unwrap_or(json!(0)));
                    at.insert("end".to_owned(), next.get("offset").cloned().unwrap_or(json!(0)));
                    d.current = Some(Value::Object(at));
                }
                self.find_next();
            }
        }
    }

    /// Replace All: every match of the scope, as one undo step.
    pub(crate) fn replace_all(&mut self) {
        let Some((with, what)) = self.ui.find.dialog.as_ref().map(|d| (d.with.clone(), d.what.clone())) else { return };
        let mut params = match self.find_params() {
            Ok(p) => p,
            Err(words) => return self.set_line(words, true),
        };
        params.insert("with".to_owned(), json!(with));
        remember(&mut self.ui.find.history, &what);
        remember(&mut self.ui.find.with_history, &with);
        match self.execute("edit.replace", &Value::Object(params)) {
            Err(e) => self.set_line(e.to_string(), true),
            Ok(done) => {
                let replaced = done.get("replaced").and_then(Value::as_u64).unwrap_or(0);
                let shapes = done.get("shapes").and_then(Value::as_u64).unwrap_or(0);
                let left = done.get("skipped_total").and_then(Value::as_u64).unwrap_or(0);
                // The matches on locked layers are said apart from the others left: the engine counts them in `locked`.
                let locked = done.get("locked").and_then(Value::as_u64).unwrap_or(0).min(left);
                let mut line = match replaced {
                    0 if left == 0 => format!("Nothing to replace: can’t find “{}”.", shown(&what, 60)),
                    0 => String::new(),
                    1 => "Replaced 1 match.".to_owned(),
                    n => format!("Replaced {n} matches in {shapes} shape{}.", if shapes == 1 { "" } else { "s" }),
                };
                let mut add = |words: String| line = if line.is_empty() { words } else { format!("{line} {words}") };
                if left > locked {
                    let locked_why = diacraft_engine::draw::find::Why::Locked.words();
                    let why = done
                        .get("skipped")
                        .and_then(Value::as_array)
                        .and_then(|list| list.iter().filter_map(|m| m.get("why").and_then(Value::as_str)).find(|why| *why != locked_why))
                        .unwrap_or("");
                    // (The answer lists the first hundred matches left: one past them has its reason there.)
                    add(if why.is_empty() { format!("{} not replaced.", left - locked) } else { format!("{} not replaced: {why}.", left - locked) });
                }
                if locked > 0 {
                    add(format!("{locked} match{} on locked layers left.", if locked == 1 { "" } else { "es" }));
                }
                self.set_line(line, replaced == 0);
                if let Some(d) = self.ui.find.dialog.as_mut() {
                    d.current = None;
                }
            }
        }
    }

    /// Where the dialog first stands: the top right of the drawing area.
    fn find_default_pos(&self) -> egui::Pos2 {
        self.ui.canvas_rect.map_or(pos2(260.0, 140.0), |r| pos2((r.right() - 400.0).max(r.left() + 8.0), r.top() + 24.0))
    }

    /// The dialog over the window, when it is open.
    pub(crate) fn find_dialog_window(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut d) = self.ui.find.dialog.take() else { return };
        let has_selection = !self.ui.edit.selection().is_empty();
        let history = self.ui.find.history.clone();
        let with_history = self.ui.find.with_history.clone();
        let mut open = true;
        let mut action: Option<Action> = None;
        let mut close = false;
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let default = self.find_default_pos();
        let frame = egui::Frame::new()
            .fill(t.ribbon_fill)
            .stroke(egui::Stroke::new(1.0, t.ribbon_border))
            .corner_radius(egui::CornerRadius::same(6))
            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.page_shadow })
            .inner_margin(egui::Margin::same(12));
        let shown = egui::Window::new(RichText::new(d.mode.title()).font(theme::font(size::TAB + 1.0)).color(t.text))
            .id(Id::unique("find_dialog_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(default)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(DIALOG_W);
                ui.spacing_mut().item_spacing = vec2(6.0, 5.0);
                let focus = std::mem::take(&mut d.focus);
                // Find what, with the searches earlier and the Special list.
                text_row(ui, &t, "Find what:", "find.what", &mut d.what, &history, focus, &mut seen, &mut action, &mut close);
                ui.horizontal(|ui| {
                    ui.add_space(86.0);
                    let special = ui.menu_button(RichText::new("Special").font(theme::font(size::LABEL)).color(t.text), |ui| {
                        for (words, code) in SPECIALS {
                            let item = ui.button(RichText::new(format!("{words}  ({code})")).font(theme::font(size::LABEL)));
                            seen.push((format!("find.special.{code}"), item.rect, true));
                            if item.clicked() {
                                d.what.push_str(code);
                                d.focus = true;
                                d.current = None;
                                ui.close();
                            }
                        }
                    });
                    seen.push(("find.special".to_owned(), special.response.rect, true));
                });
                if d.mode == Mode::Replace {
                    text_row(ui, &t, "Replace with:", "find.with", &mut d.with, &with_history, false, &mut seen, &mut action, &mut close);
                }
                ui.add_space(2.0);
                group(ui, &t, if d.mode == Mode::Find { "Search in" } else { "Search" }, |ui| {
                    ui.horizontal(|ui| {
                        for scope in Scope::ALL {
                            let enabled = scope != Scope::Selection || has_selection || d.scope == Scope::Selection;
                            let r = ui.add_enabled(
                                enabled,
                                egui::RadioButton::new(d.scope == scope, RichText::new(scope.caption()).font(theme::font(size::LABEL)).color(t.text)),
                            );
                            seen.push((format!("find.scope.{}", scope.name()), r.rect, enabled));
                            if r.clicked() && d.scope != scope {
                                d.scope = scope;
                                d.current = None;
                            }
                        }
                    });
                    if d.mode == Mode::Find {
                        ui.horizontal_wrapped(|ui| {
                            for (on, caption, name) in [
                                (&mut d.in_text, "Shape text", "find.in.text"),
                                (&mut d.in_data, "Shape data", "find.in.data"),
                                (&mut d.in_name, "Shape name", "find.in.name"),
                                (&mut d.in_user, "User-defined cell", "find.in.user"),
                            ] {
                                let r = ui.checkbox(on, RichText::new(caption).font(theme::font(size::LABEL)).color(t.text));
                                seen.push((name.to_owned(), r.rect, true));
                                if r.changed() {
                                    d.current = None;
                                }
                            }
                        });
                    }
                });
                group(ui, &t, "Options", |ui| {
                    ui.horizontal(|ui| {
                        let case = ui.checkbox(&mut d.match_case, RichText::new("Match case").font(theme::font(size::LABEL)).color(t.text));
                        seen.push(("find.match_case".to_owned(), case.rect, true));
                        let whole =
                            ui.checkbox(&mut d.whole_words, RichText::new("Find whole words only").font(theme::font(size::LABEL)).color(t.text));
                        seen.push(("find.whole_words".to_owned(), whole.rect, true));
                        if case.changed() || whole.changed() {
                            d.current = None;
                        }
                    });
                });
                // The Found in line, which also says what a button did or why it could not.
                let colour = if d.line.problem { t.notice_text } else { t.text_dim };
                let words = if d.line.text.is_empty() { "Found in:".to_owned() } else { d.line.text.clone() };
                let said = ui.add(egui::Label::new(RichText::new(&words).font(theme::font(size::LABEL)).color(colour)).wrap());
                said.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &words));
                seen.push(("find.found_in".to_owned(), said.rect, true));
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let mut buttons: Vec<(&str, &str, Option<Action>)> = Vec::new();
                    if d.mode == Mode::Replace {
                        buttons.push(("Replace", "find.replace", Some(Action::Replace)));
                        buttons.push(("Replace All", "find.replace_all", Some(Action::ReplaceAll)));
                    }
                    buttons.push(("Find Next", "find.next", Some(Action::Next)));
                    buttons.push(("Close", "find.close", None));
                    for (label, name, what) in buttons {
                        let ready = !d.what.is_empty() || what.is_none();
                        let b =
                            ui.add_enabled(ready, egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(78.0, 24.0)));
                        seen.push((name.to_owned(), b.rect, ready));
                        if b.clicked() {
                            match what {
                                Some(a) => action = Some(a),
                                None => close = true,
                            }
                        }
                    }
                });
            });
        if let Some(window) = shown {
            seen.push(("find.window".to_owned(), window.response.rect, true));
        }
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !open || close {
            self.ui.find.dialog = None;
            return;
        }
        self.ui.find.dialog = Some(d);
        match action {
            Some(Action::Next) => self.find_next(),
            Some(Action::Replace) => self.replace_current(),
            Some(Action::ReplaceAll) => self.replace_all(),
            None => {}
        }
    }
}

/// A titled group of controls, with a line of its name above.
fn group(ui: &mut Ui, t: &Tokens, name: &str, body: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(name).font(theme::font(size::LABEL)).color(t.text_dim));
    ui.indent(name, |ui| body(ui));
}

/// A caption, a box with a button for the earlier entries: Enter in the box is Find Next, and Escape closes the dialog.
#[allow(clippy::too_many_arguments)]
fn text_row(
    ui: &mut Ui,
    t: &Tokens,
    caption: &str,
    name: &str,
    text: &mut String,
    history: &[String],
    focus: bool,
    seen: &mut Vec<(String, egui::Rect, bool)>,
    action: &mut Option<Action>,
    close: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(80.0, 20.0), egui::Label::new(RichText::new(caption).font(theme::font(size::LABEL)).color(t.text)));
        let edit = ui.add(egui::TextEdit::singleline(text).desired_width(200.0).char_limit(FIELD_CHARS).font(theme::font(size::LABEL)));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, name));
        seen.push((name.to_owned(), edit.rect, true));
        if focus {
            edit.request_focus();
        }
        if edit.lost_focus() {
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                *close = true;
            } else if enter && name == "find.what" {
                *action = Some(Action::Next);
                edit.request_focus();
            }
        }
        let (rect, button) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
        if button.hovered() {
            ui.painter().rect_filled(rect, 3.0, t.hover);
        }
        crate::icons::chevron(ui.painter(), rect.center(), 3.5, t.text_dim);
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Earlier entries"));
        seen.push((format!("{name}.earlier"), rect, true));
        egui::Popup::menu(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            if history.is_empty() {
                ui.label(RichText::new("Nothing earlier").font(theme::font(size::LABEL)).color(t.text_dim));
            }
            for (i, entry) in history.iter().enumerate() {
                let item = ui.button(RichText::new(shown(entry, 40)).font(theme::font(size::LABEL)));
                seen.push((format!("{name}.history.{i}"), item.rect, true));
                if item.clicked() {
                    text.clone_from(entry);
                }
            }
        });
    });
}
