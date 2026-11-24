//! Home > Clipboard > Format Painter: copies the look of the shape selected and puts it on the next shape the
//! pointer presses.
//!
//! **The painter is a mode of the drawing area, as a tool is**: a press on the button picks the formatting up, with the pointer a paint cursor over the page,
//! and the next press on a shape paints it; the painter then turns itself off. A double click on the button keeps it on, so that several shapes can be painted in
//! turn (the one source for it is a tutorial, `community-report`), until Escape, the Pointer Tool or any other tool puts it down.
//!
//! **What is copied** is what `shape.formatting` says the first shape selected is: the fill, the line (its colour, weight, dash pattern, cap, compound type and two ends), the
//! transparency of both, a pattern fill and the rounding of its corners, which `shape.format` sets, the gradients of the fill and the line, which `shape.gradient` sets, and the
//! shadow, which `shape.shadow` sets (a source with no shadow takes the shadow away), and the face, size, styles, colour and alignment of its text and where
//! the text sits in its block, which `text.format` sets. Nothing of the shape's data or its place is copied. **A paint is one undo step**: the commands run together as one
//! `command.batch`.
//!
//! **With two shapes or more selected** the first is the source and the others are painted at once, as the page says ("select the source shape, hold Ctrl and click
//! the shapes to change, then choose Format Painter"): no mode is turned on, unless the button was double-clicked.
//!
//! **A shape that is a member of a group cannot be copied from** (the page says so), and a press on a member paints the group it is in: the format goes to every shape
//! of the group, as `shape.format` does for a group.
//!
//! The window command is `format.painter` (`commands.rs`); the button, the key (Ctrl+Shift+P) and the control channel all reach it.

use diacraft_engine::{EngineError, Result};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::edit::{Cx, HIT_REACH};

/// What `shape.formatting` answers for a shape and `shape.format` takes: the fill and line, the weight and the pattern of the line, its two ends and their sizes, the corners, the
/// transparency of the fill and the line, and the cap and the compound type of the line (the fill's pattern is picked apart below: `shape.format` takes the hatches only).
const SHAPE_KEYS: [&str; 13] = [
    "fill",
    "line",
    "weight",
    "pattern",
    "begin_arrow",
    "end_arrow",
    "begin_arrow_size",
    "end_arrow_size",
    "rounding",
    "fill_transparency",
    "line_transparency",
    "line_cap",
    "compound",
];

/// What `shape.formatting` answers for a shape and `text.format` takes: the face, the size, the four styles, the colour, the alignment of the lines and of the block.
const TEXT_KEYS: [&str; 9] = ["font", "size", "bold", "italic", "underline", "strike", "color", "align", "valign"];

/// The most groups a press climbs through to the one that holds the shape pressed: a drawing's own nesting is far less, and a loop of parents (a bad file) ends.
const MAX_DEPTH: usize = 64;

/// What the painter holds: the look it picked up, and whether it stays on after a paint.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painter {
    /// The parameters of `shape.format` and of `text.format` that make a shape look as the source did (neither holds `ids` or `page`).
    shape: Map<String, Value>,
    text: Map<String, Value>,
    /// The commands after those two that make a shape's gradients and shadow as the source's are, with their parameters (`ids` and `page` not among them).
    extra: Vec<(&'static str, Map<String, Value>)>,
    /// The shape the look was taken from, and its page.
    from: (u32, u32),
    /// A double click: the painter stays on after a paint, until Escape or another tool.
    keep: bool,
}

impl Painter {
    /// Whether the painter stays on after a paint.
    pub(crate) fn keeps(&self) -> bool {
        self.keep
    }

    /// The shape the look was taken from.
    pub(crate) fn source(&self) -> u32 {
        self.from.0
    }
}

/// `shape.formatting`'s look of one shape as the parameters of the two commands that set it. A key the look has no value for (a face the cells do not name: `null`) is left
/// out, so that a paint never writes a value the source did not show.
fn held_from(look: &Value) -> (Map<String, Value>, Map<String, Value>) {
    let pick = |keys: &[&str]| -> Map<String, Value> {
        keys.iter().filter_map(|k| look.get(*k).filter(|v| !v.is_null()).map(|v| ((*k).to_owned(), v.clone()))).collect()
    };
    let mut shape = pick(&SHAPE_KEYS);
    // A hatch or stipple, or a solid fill, is a pattern `shape.format` takes (with the colour between its marks); a gradient the old way and a master are not.
    if look.get("fill_pattern").and_then(Value::as_u64).is_some_and(|n| (1..=24).contains(&n)) {
        for key in ["fill_pattern", "fill_background"] {
            if let Some(v) = look.get(key).filter(|v| !v.is_null()) {
                shape.insert(key.to_owned(), v.clone());
            }
        }
    }
    // A line that has no weight (a shape with no line) holds none: `shape.format` takes a weight above 0.
    if shape.get("weight").and_then(Value::as_f64).is_some_and(|w| w <= 0.0) {
        shape.remove("weight");
    }
    (shape, pick(&TEXT_KEYS))
}

/// The commands that give a shape the gradients and the shadow the look has: a gradient the source paints is made again (`on`, its layout, angle and stops), and a shadow too; a
/// source with no shadow takes the shadow away. A source with no gradient needs no command: the colour `shape.format` sets turns the target's gradient off.
fn extras_from(look: &Value) -> Vec<(&'static str, Map<String, Value>)> {
    let mut out = Vec::new();
    for (key, target) in [("gradient", "fill"), ("line_gradient", "line")] {
        let Some(g) = look.get(key).and_then(Value::as_object) else { continue };
        let mut params = Map::new();
        params.insert("target".to_owned(), json!(target));
        params.insert("on".to_owned(), json!(true));
        for from in ["direction", "angle", "rotate_with_shape", "stops"] {
            if let Some(v) = g.get(from) {
                params.insert(from.to_owned(), v.clone());
            }
        }
        out.push(("shape.gradient", params));
    }
    let mut shadow = Map::new();
    match look.get("shadow").and_then(Value::as_object) {
        Some(s) => {
            shadow.insert("on".to_owned(), json!(true));
            for (from, to) in [
                ("color", "color"),
                ("transparency", "transparency"),
                ("size", "size"),
                ("blur", "blur"),
                ("angle", "angle"),
                ("distance", "distance"),
            ] {
                if let Some(v) = s.get(from) {
                    shadow.insert(to.to_owned(), v.clone());
                }
            }
        }
        None => {
            shadow.insert("on".to_owned(), json!(false));
        }
    }
    out.push(("shape.shadow", shadow));
    out
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "format.painter".to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// Whether the Format Painter is on.
    pub fn format_painter_on(&self) -> bool {
        self.ui.edit.painter.is_some()
    }

    /// Whether the painter stays on after a paint (a double click), `None` while it is off: for `window_state`.
    pub(crate) fn format_painter_state(&self) -> Option<Value> {
        self.ui.edit.painter.as_ref().map(|p| json!({"keep": p.keeps(), "from": p.source()}))
    }

    /// Whether the command `format.painter` can run now: it needs a shape to copy, and it can always be put down.
    pub(crate) fn format_painter_enabled(&self) -> std::result::Result<(), String> {
        if self.session().document().is_none() {
            Err("no document open".to_owned())
        } else if self.ui.edit.painter.is_none() && self.ui.edit.selection().is_empty() {
            Err("nothing is selected".to_owned())
        } else {
            Ok(())
        }
    }

    /// `format.painter`: picks the look of the first shape selected up (`on` true, or none while it is off), or puts the painter down (`on` false, or none while it
    /// is on); `keep` keeps it on after a paint; `to` paints those shapes with what it holds, as a press on each would.
    pub(crate) fn format_painter_command(&mut self, on: Option<bool>, keep: bool, to: Option<Vec<u32>>) -> Result<()> {
        if let Some(ids) = to {
            return self.paint_ids(&ids, keep);
        }
        let want = on.unwrap_or(!self.format_painter_on());
        if !want {
            self.ui.edit.painter = None;
            return Ok(());
        }
        self.pick_up(keep)
    }

    /// Picks the look of the first shape selected up. With more shapes selected the others are painted at once.
    fn pick_up(&mut self, keep: bool) -> Result<()> {
        let (page, ids) = self.selection_target("format.painter")?;
        let Some(&source) = ids.first() else { return Err(bad("nothing is selected")) };
        // The painter is a mode: a tool that was on is put down.
        self.set_tool(crate::edit::Tool::Pointer);
        self.ensure_scene(page);
        if self.ui.edit.scene().and_then(|s| s.get(source)).is_some_and(|item| item.parent.is_some()) {
            return Err(bad("the Format Painter cannot copy from a shape that is in a group: select the group, or a shape that is not in one"));
        }
        let answer = self.execute("shape.formatting", &json!({"ids": [source], "page": page}))?;
        let look = answer
            .get("shapes")
            .and_then(Value::as_array)
            .and_then(|shapes| shapes.first())
            .ok_or_else(|| bad("the shape has no formatting to copy"))?;
        let (shape, text) = held_from(look);
        let painter = Painter { shape, text, extra: extras_from(look), from: (source, page), keep };
        let others: Vec<u32> = ids.iter().copied().filter(|id| *id != source).collect();
        if others.is_empty() {
            self.ui.edit.painter = Some(painter);
            return Ok(());
        }
        // Source first, the others painted: no mode unless the button was double-clicked.
        self.paint(&painter, page, &others)?;
        self.ui.edit.painter = keep.then_some(painter);
        Ok(())
    }

    /// Paints `ids` of the page on view with what the painter holds, and puts it down unless it stays on.
    fn paint_ids(&mut self, ids: &[u32], keep: bool) -> Result<()> {
        let Some(painter) = self.ui.edit.painter.clone() else {
            return Err(bad("the Format Painter is not on: choose it first, with a shape selected, and then paint"));
        };
        let page = self.current_page().ok_or_else(|| bad("no document open"))?;
        self.paint(&painter, page, ids)?;
        if !(keep || painter.keeps()) {
            self.ui.edit.painter = None;
        }
        Ok(())
    }

    /// One `command.batch`: `shape.format`, `text.format`, the gradients and the shadow of `ids` with the look held, as one undo step. A look with nothing in one of them leaves it out.
    fn paint(&mut self, painter: &Painter, page: u32, ids: &[u32]) -> Result<()> {
        let mut steps: Vec<Value> = Vec::with_capacity(5);
        let own = [("shape.format", &painter.shape), ("text.format", &painter.text)];
        let extra = painter.extra.iter().map(|(command, held)| (*command, held));
        for (command, held) in own.into_iter().chain(extra) {
            if held.is_empty() {
                continue;
            }
            let mut params = held.clone();
            params.insert("ids".to_owned(), json!(ids));
            params.insert("page".to_owned(), json!(page));
            steps.push(json!({"command": command, "params": params}));
        }
        if steps.is_empty() {
            return Err(bad("the shape has no formatting to copy"));
        }
        self.execute("command.batch", &json!({"commands": steps, "label": "Format Painter"})).map(|_| ())
    }

    /// A press on the drawing area while the painter is on: the shape under the pointer (the group it is in, as a group is painted whole) is painted; a press on the
    /// bare page changes nothing. **The press is the painter's either way**, so no drag, no rubber band and no selection start under it.
    pub(crate) fn paint_press(&mut self, cx: &Cx, p: crate::edit::geom::P) {
        let Some(scene) = self.ui.edit.scene() else { return };
        let Some(mut target) = scene.hit(p, cx.reach(HIT_REACH)).map(|item| item.id) else { return };
        for _ in 0..MAX_DEPTH {
            match scene.get(target).and_then(|item| item.parent) {
                Some(parent) => target = parent,
                None => break,
            }
        }
        if let Err(e) = self.paint_ids(&[target], false) {
            self.set_notice(&e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_is_split_between_the_two_commands_and_leaves_out_what_it_has_no_value_for() {
        let look = json!({
            "id": 3, "fill": "#cfe2f3", "line": "none", "weight": 0.0, "pattern": 1, "begin_arrow": 0, "end_arrow": 4, "begin_arrow_size": 2, "end_arrow_size": 5, "rounding": 0.1,
            "font": null, "size": 12.0, "bold": true, "italic": false, "underline": false, "strike": false, "color": "#000000", "align": "center", "valign": "middle",
        });
        let (shape, text) = held_from(&look);
        assert_eq!(shape.get("fill"), Some(&json!("#cfe2f3")));
        assert_eq!(shape.get("line"), Some(&json!("none")), "a line that paints nothing is held, so that it is painted away");
        assert!(!shape.contains_key("weight"), "a weight of 0 is not one `shape.format` takes: {shape:?}");
        assert_eq!(shape.get("end_arrow"), Some(&json!(4)));
        assert_eq!(
            (shape.get("begin_arrow_size"), shape.get("end_arrow_size")),
            (Some(&json!(2)), Some(&json!(5))),
            "the size of a line end is carried too"
        );
        assert!(!shape.contains_key("id"), "the shape's own id is not a formatting: {shape:?}");
        assert!(!text.contains_key("font"), "a face the cells do not name is not copied: {text:?}");
        assert_eq!(text.get("bold"), Some(&json!(true)));
        assert_eq!(text.get("valign"), Some(&json!("middle")));
        assert_eq!(text.len(), 8);
    }

    #[test]
    fn a_hatch_a_transparency_a_gradient_and_a_shadow_are_held_and_a_missing_shadow_is_painted_away() {
        let look = json!({
            "fill": "#336699", "line": "#cc0000", "weight": 1.5, "pattern": 1, "fill_transparency": 25.0, "line_transparency": 10.0, "line_cap": "square", "compound": "double",
            "fill_pattern": 6, "fill_background": "#ffff00",
            "gradient": {"direction": 0, "kind": "linear", "origin": "", "angle": 90.0, "rotate_with_shape": true, "stops": [{"position": 0.0, "color": "#ff0000", "transparency": 0.0}]},
            "line_gradient": null,
            "shadow": {"color": "#000000", "transparency": 60.0, "size": 100.0, "blur": 4.0, "angle": 45.0, "distance": 4.0, "kind": 1},
        });
        let (shape, _) = held_from(&look);
        assert_eq!(shape.get("fill_pattern"), Some(&json!(6)));
        assert_eq!(shape.get("fill_background"), Some(&json!("#ffff00")));
        assert_eq!((shape.get("line_cap"), shape.get("compound")), (Some(&json!("square")), Some(&json!("double"))));
        assert_eq!(shape.get("fill_transparency"), Some(&json!(25.0)));
        let extra = extras_from(&look);
        assert_eq!(extra.iter().map(|(c, _)| *c).collect::<Vec<_>>(), ["shape.gradient", "shape.shadow"], "no gradient of the line to make");
        assert_eq!(extra[0].1.get("target"), Some(&json!("fill")));
        assert_eq!(extra[0].1.get("stops").map(|s| s.as_array().map(Vec::len)), Some(Some(1)));
        assert_eq!(extra[1].1.get("angle"), Some(&json!(45.0)));
        // A source with no shadow: the shadow of the shape painted is taken away. A legacy pattern the commands do not take is left out.
        let plain = json!({"fill": "#ffffff", "fill_pattern": 30, "gradient": null, "line_gradient": null, "shadow": null});
        let (shape, _) = held_from(&plain);
        assert!(!shape.contains_key("fill_pattern"));
        assert_eq!(extras_from(&plain), vec![("shape.shadow", json!({"on": false}).as_object().cloned().unwrap_or_default())]);
    }
}
