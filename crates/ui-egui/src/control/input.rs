//! `ui.elements` and the synthetic input methods: how a test or an agent finds a control and presses it.
//!
//! The input is egui's own: pointer and keyboard events queued here and handed to egui at the start of a frame
//! (`DiacraftApp::raw_input_hook`), so it reaches every widget the way a person's would. A press and its release (and each key
//! press and release) land in separate frames, so a control has seen the pointer over it before it is pressed. Nothing here
//! sends input to the desktop: only this window gets it.

use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, pos2, vec2};
use serde_json::{Map, Value, json};

use super::{ControlReply, only_keys};
use crate::app::DiacraftApp;

/// Most events waiting to be handed to egui at once. A request that would go over is refused whole.
pub const MAX_SYNTHETIC_EVENTS: usize = 1024;

/// Most steps of a drag, and most clicks in a `count`.
const MAX_DRAG_STEPS: u64 = 256;
const MAX_CLICKS: u64 = 16;

/// The smallest and the largest side `ui.resize` asks for, in points.
const MIN_SIDE: f64 = 200.0;
const MAX_SIDE: f64 = 8192.0;

/// The most controls `ui.elements` lists.
const MAX_LISTED: usize = 2000;

fn number(params: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("`{key}` must be a finite number")),
    }
}

fn whole(params: &Map<String, Value>, key: &str, default: u64, most: u64) -> Result<u64, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().filter(|n| *n <= most).map(|n| n.max(1)).ok_or_else(|| format!("`{key}` must be a whole number from 0 to {most}")),
    }
}

fn modifiers(params: &Map<String, Value>) -> Modifiers {
    let on = |k: &str| params.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (ctrl, shift, alt) = (on("ctrl"), on("shift"), on("alt"));
    Modifiers { alt, ctrl, shift, mac_cmd: false, command: ctrl }
}

fn key_named(name: &str) -> Option<Key> {
    Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(Key::Enter),
        "esc" | "escape" => Some(Key::Escape),
        "delete" => Some(Key::Delete),
        "backspace" => Some(Key::Backspace),
        "left" => Some(Key::ArrowLeft),
        "right" => Some(Key::ArrowRight),
        "up" => Some(Key::ArrowUp),
        "down" => Some(Key::ArrowDown),
        "space" => Some(Key::Space),
        "tab" => Some(Key::Tab),
        "plus" => Some(Key::Plus),
        "minus" => Some(Key::Minus),
        _ => None,
    })
}

impl DiacraftApp {
    /// `ui.elements {prefix?}` → the controls the last frame drew: `[{name, x, y, w, h, enabled}]`.
    pub(super) fn method_elements(&self, params: &Map<String, Value>) -> ControlReply {
        only_keys(params, "ui.elements", &["prefix"])?;
        let prefix = match params.get("prefix") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err("`prefix` must be text".to_owned()),
        };
        let list: Vec<Value> = self
            .elements()
            .iter()
            .filter(|e| e.name.starts_with(prefix))
            .take(MAX_LISTED)
            .map(|e| json!({"name": e.name, "x": e.rect.min.x, "y": e.rect.min.y, "w": e.rect.width(), "h": e.rect.height(), "enabled": e.enabled}))
            .collect();
        Ok(Value::Array(list))
    }

    /// Where a request points: a control by `element` name (its middle), or `x` and `y`.
    fn target(&self, params: &Map<String, Value>, element_key: &str, x_key: &str, y_key: &str) -> Result<Pos2, String> {
        if let Some(v) = params.get(element_key).filter(|v| !v.is_null()) {
            let Value::String(name) = v else { return Err(format!("`{element_key}` must be text: the name of a control from `ui.elements`")) };
            return match self.element(name) {
                Some(e) => Ok(e.rect.center()),
                None => Err(format!("no control named `{}` was drawn in the last frame (see `ui.elements`)", diacraft_engine::shown(name, 64))),
            };
        }
        match (number(params, x_key)?, number(params, y_key)?) {
            (Some(x), Some(y)) => Ok(pos2(x as f32, y as f32)),
            _ => Err(format!("give `{x_key}` and `{y_key}` (screen points), or `{element_key}`")),
        }
    }

    fn reserve(&mut self, more: usize) -> Result<(), String> {
        if more > MAX_SYNTHETIC_EVENTS.saturating_sub(self.synthetic.len()) {
            return Err(format!("synthetic input queue limit ({MAX_SYNTHETIC_EVENTS} events); retry after pending input drains"));
        }
        self.synthetic.try_reserve(more).map_err(|e| format!("cannot reserve synthetic input: {e}"))
    }

    /// The six input methods. Nothing is queued unless the whole request is good.
    pub(super) fn method_input(&mut self, ctx: &egui::Context, method: &str, params: &Map<String, Value>) -> ControlReply {
        let mods = modifiers(params);
        match method {
            "ui.move" => {
                only_keys(params, method, &["x", "y", "element"])?;
                let at = self.target(params, "element", "x", "y")?;
                self.reserve(1)?;
                self.synthetic.push(Event::PointerMoved(at));
            }
            "ui.click" => {
                only_keys(params, method, &["x", "y", "element", "button", "count", "ctrl", "shift", "alt"])?;
                let at = self.target(params, "element", "x", "y")?;
                let count = whole(params, "count", 1, MAX_CLICKS)?;
                let button = button(params);
                self.reserve(2 + 2 * count as usize)?;
                self.synthetic.push(Event::PointerMoved(at));
                for _ in 0..count {
                    self.synthetic.push(Event::PointerButton { pos: at, button, pressed: true, modifiers: mods });
                    self.synthetic.push(Event::PointerButton { pos: at, button, pressed: false, modifiers: mods });
                }
            }
            "ui.drag" => {
                only_keys(params, method, &["x", "y", "element", "toX", "toY", "steps", "button", "ctrl", "shift", "alt", "hold"])?;
                // `hold`: the button stays down where the drag ends, until `ui.release`, so that a client can look at a drag in progress.
                let hold = match params.get("hold") {
                    None | Some(Value::Null) | Some(Value::Bool(false)) => false,
                    Some(Value::Bool(true)) => true,
                    Some(_) => return Err("`hold` must be true or false".to_owned()),
                };
                let from = self.target(params, "element", "x", "y")?;
                let (Some(tx), Some(ty)) = (number(params, "toX")?, number(params, "toY")?) else { return Err("give `toX` and `toY`".to_owned()) };
                let to = pos2(tx as f32, ty as f32);
                let steps = whole(params, "steps", 8, MAX_DRAG_STEPS)?;
                let button = button(params);
                self.reserve(3 + steps as usize)?;
                self.synthetic.push(Event::PointerMoved(from));
                self.synthetic.push(Event::PointerButton { pos: from, button, pressed: true, modifiers: mods });
                for i in 1..=steps {
                    self.synthetic.push(Event::PointerMoved(from + (to - from) * (i as f32 / steps as f32)));
                }
                if !hold {
                    self.synthetic.push(Event::PointerButton { pos: to, button, pressed: false, modifiers: mods });
                }
            }
            "ui.release" => {
                // The end of a drag that `ui.drag {hold}` left with the button down: a move to the place given (if one is), then the release there.
                only_keys(params, method, &["x", "y", "element", "button", "ctrl", "shift", "alt"])?;
                let given = params.get("element").is_some_and(|v| !v.is_null()) || params.get("x").is_some_and(|v| !v.is_null());
                let at = if given { Some(self.target(params, "element", "x", "y")?) } else { None };
                let button = button(params);
                self.reserve(2)?;
                if let Some(at) = at {
                    self.synthetic.push(Event::PointerMoved(at));
                }
                // Where the pointer is: the place given, else the last place queued, else where egui last saw it.
                let queued = self.synthetic.iter().rev().find_map(|e| if let Event::PointerMoved(p) = e { Some(*p) } else { None });
                let pos = at.or(queued).or_else(|| ctx.input(|i| i.pointer.latest_pos())).unwrap_or(Pos2::ZERO);
                self.synthetic.push(Event::PointerButton { pos, button, pressed: false, modifiers: mods });
            }
            "ui.wheel" => {
                only_keys(params, method, &["x", "y", "element", "dx", "dy", "unit", "ctrl", "shift", "alt"])?;
                let at = self.target(params, "element", "x", "y")?;
                let delta = vec2(number(params, "dx")?.unwrap_or(0.0) as f32, number(params, "dy")?.unwrap_or(0.0) as f32)
                    .clamp(egui::Vec2::splat(-100.0), egui::Vec2::splat(100.0));
                let unit = if params.get("unit").and_then(Value::as_str) == Some("point") { MouseWheelUnit::Point } else { MouseWheelUnit::Line };
                self.reserve(2)?;
                self.synthetic.push(Event::PointerMoved(at));
                self.synthetic.push(Event::MouseWheel {
                    unit,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: mods,
                    source: egui::MouseWheelSource::Unknown,
                });
            }
            "ui.key" => {
                only_keys(params, method, &["key", "ctrl", "shift", "alt"])?;
                let Some(key) = params.get("key").and_then(Value::as_str).and_then(key_named) else {
                    return Err("unknown or missing `key`".to_owned());
                };
                self.reserve(2)?;
                self.synthetic.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods });
                self.synthetic.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods });
            }
            _ => {
                only_keys(params, method, &["text"])?;
                let Some(Value::String(text)) = params.get("text") else { return Err("`ui.text` needs a string `text`".to_owned()) };
                self.reserve(1)?;
                self.synthetic.push(Event::Text(text.chars().take(1000).collect()));
            }
        }
        ctx.request_repaint();
        Ok(Value::Null)
    }

    /// `ui.resize {width, height}` asks the desktop for an inner window size, in points, from 200 to 8,192 each way.
    pub(super) fn method_resize(&mut self, ctx: &egui::Context, params: &Map<String, Value>) -> ControlReply {
        only_keys(params, "ui.resize", &["width", "height"])?;
        let side = |key: &str| match number(params, key)? {
            Some(n) if (MIN_SIDE..=MAX_SIDE).contains(&n) => Ok(n as f32),
            _ => Err(format!("`{key}` must be a number of points from {MIN_SIDE} to {MAX_SIDE}")),
        };
        let size = vec2(side("width")?, side("height")?);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.request_repaint();
        Ok(Value::Null)
    }

    /// How many of the queued events go to egui in the next frame: up to and including the first press, release or key, so the
    /// events after it meet a frame that has seen the pointer where it was. **A release that comes after moves is left for the frame
    /// after them**: a drag is then a frame of the pointer moving with the button down and a frame of the release, as a hand makes it,
    /// and a widget that waits for a drag to begin (a tile of the Shapes window) sees one.
    pub(crate) fn synthetic_frame_len(&self) -> usize {
        let Some(boundary) = self.synthetic.iter().position(|e| matches!(e, Event::PointerButton { .. } | Event::Key { .. })) else {
            return self.synthetic.len();
        };
        let releases = matches!(self.synthetic.get(boundary), Some(Event::PointerButton { pressed: false, .. }));
        let moved_first = self.synthetic.iter().take(boundary).any(|e| matches!(e, Event::PointerMoved(_)));
        if releases && moved_first { boundary } else { boundary + 1 }
    }
}

fn button(params: &Map<String, Value>) -> PointerButton {
    match params.get("button").and_then(Value::as_str) {
        Some("right" | "secondary") => PointerButton::Secondary,
        Some("middle") => PointerButton::Middle,
        _ => PointerButton::Primary,
    }
}
