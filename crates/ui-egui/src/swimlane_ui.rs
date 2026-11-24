//! The Cross-Functional Flowchart in the window: the contextual tab that shows while the page on view has swimlanes, and the right-click rows of a lane.
//!
//! **The window holds no swimlane state of its own.** What the tab knows (that the page has a structure, how its lanes lie, which parts are selected and where each lane stands in its list) is
//! read from the engine with `swimlane.info` for the shapes selected, once for each revision of the drawing, page and selection ([`DiacraftApp::refresh_swimlane_view`]); and every press is a
//! command of the engine (`swimlane.insert`, `swimlane.insert_phase`, `swimlane.orientation`, `swimlane.rotate_label`, `swimlane.move`) sent by the window command `window.swimlaneDo`, which adds
//! what only the window knows: the lane, the phase or the structure selected. A lane dragged by its header is reordered by the engine when it is let go (`shape.move`), so the window has
//! no drag of its own for it.

use diacraft_engine::EngineError;
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::ribbon::spec::Mark;

/// The most shapes the tab asks the engine about at once (a selection of more is not one the tab is read for).
const MAX_ASKED: usize = 4_096;

/// What the tab knows of the page and the selection.
#[derive(Debug, Default)]
pub struct SwimlaneView {
    /// The drawing's revision, the page and the selection this was read for.
    key: Option<(u64, u32, Vec<u32>)>,
    /// The page has a structure of lanes.
    pub(crate) present: bool,
    /// How the lanes of the first structure lie: `horizontal` or `vertical`.
    pub(crate) orientation: Option<String>,
    /// The shapes selected that are parts of a structure: the shape, its role (`frame`, `lanes`, `phases`, `lane` or `phase`) and the list of lanes of its structure.
    pub(crate) selected: Vec<(u32, String, u32)>,
    /// The lanes of each structure, in order: the list of lanes and the lane ids.
    lanes: Vec<(u32, Vec<u32>)>,
    /// The first structure's frame and list of lanes, for a command with nothing selected.
    first: Option<(Option<u32>, u32)>,
}

fn refused(cmd: &str, msg: &str) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.to_owned() }
}

fn whole(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

impl DiacraftApp {
    /// Reads what the tab shows, if the drawing, the page or the selection has changed since it was read. Called before the ribbon is drawn.
    pub(crate) fn refresh_swimlane_view(&mut self) {
        let Some(page) = self.current_page() else {
            self.ui.swimlane = SwimlaneView::default();
            return;
        };
        let selection: Vec<u32> = self.selection().iter().copied().take(MAX_ASKED).collect();
        let key = (self.session.revision(), page, selection.clone());
        if self.ui.swimlane.key.as_ref() == Some(&key) {
            return;
        }
        let mut view = SwimlaneView { key: Some(key), ..SwimlaneView::default() };
        if self.session.document().is_some()
            && let Ok(answer) = self.session.execute("swimlane.info", &json!({"ids": selection, "page": page}))
        {
            for st in answer["structures"].as_array().into_iter().flatten() {
                let Some(list) = whole(&st["lane_list"]) else { continue };
                if !view.present {
                    view.present = true;
                    view.orientation = st["orientation"].as_str().map(str::to_owned);
                    view.first = Some((whole(&st["frame"]), list));
                }
                let lanes: Vec<u32> = st["lanes"].as_array().into_iter().flatten().filter_map(|l| whole(&l["id"])).collect();
                view.lanes.push((list, lanes));
            }
            for s in answer["selected"].as_array().into_iter().flatten() {
                if let (Some(id), Some(role), Some(list)) = (whole(&s["id"]), s["role"].as_str(), whole(&s["lane_list"])) {
                    view.selected.push((id, role.to_owned(), list));
                }
            }
        }
        self.ui.swimlane = view;
    }

    /// Whether the page on view has swimlanes: what makes the Cross-Functional Flowchart tab show.
    pub(crate) fn swimlane_present(&self) -> bool {
        self.ui.swimlane.present
    }

    /// What the window knows of swimlanes, for the window state.
    pub(crate) fn swimlane_state(&self) -> Value {
        let v = &self.ui.swimlane;
        let selected: Vec<Value> = v.selected.iter().map(|(id, role, list)| json!({"id": id, "role": role, "laneList": list})).collect();
        json!({"present": v.present, "orientation": v.orientation, "selected": selected})
    }

    /// Whether the row of a menu that names an orientation is the one the structure has.
    pub(crate) fn swimlane_mark_is_current(&self, mark: Mark) -> bool {
        match mark {
            Mark::SwimlaneOrientation(word) => self.ui.swimlane.orientation.as_deref() == Some(word),
            _ => false,
        }
    }

    /// Whether a lane or a phase is selected: what gives the right-click menu its Swimlane row.
    pub(crate) fn selection_is_swimlane_part(&self) -> bool {
        self.ui.swimlane.selected.iter().any(|(_, role, _)| matches!(role.as_str(), "lane" | "phase"))
    }

    /// The first selected shape that is a part of a structure with this role.
    fn selected_part(&self, role: &str) -> Option<u32> {
        self.ui.swimlane.selected.iter().find(|(_, r, _)| r == role).map(|(id, _, _)| *id)
    }

    /// Any selected part of a structure: the shape a command that works on the whole structure names.
    fn any_selected_part(&self) -> Option<u32> {
        self.ui.swimlane.selected.first().map(|(id, _, _)| *id)
    }

    /// `window.swimlaneDo`: what the Cross-Functional Flowchart tab and the right-click rows of a lane do, to the lane, the phase or the structure selected, or to the page's first structure
    /// when none is. The new lane or phase is selected.
    pub(crate) fn swimlane_do(&mut self, op: &str, value: Option<&str>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.swimlaneDo";
        let page = self.current_page().ok_or_else(|| refused(ID, "no document open"))?;
        if !self.ui.swimlane.present {
            return Err(refused(ID, "the page has no swimlanes: File > New > Cross-Functional Flowchart starts a drawing with some"));
        }
        match op {
            "insert" => {
                let place = match value {
                    None | Some("after") => "after",
                    Some("before") => "before",
                    Some(other) => return Err(refused(ID, &format!("`value` for `insert` is before or after, not `{other}`"))),
                };
                let target = self.selected_part("lane").or_else(|| self.any_selected_part());
                let mut params = json!({"where": place, "page": page});
                if let (Some(map), Some(t)) = (params.as_object_mut(), target) {
                    map.insert("lane".to_owned(), json!(t));
                }
                let answer = self.execute("swimlane.insert", &params)?;
                if let Some(id) = whole(&answer["id"]) {
                    self.select(&[id]);
                }
            }
            "separator" => {
                let target = self.selected_part("phase").or_else(|| self.any_selected_part());
                let mut params = json!({"page": page});
                if let (Some(map), Some(t)) = (params.as_object_mut(), target) {
                    map.insert("phase".to_owned(), json!(t));
                }
                let answer = self.execute("swimlane.insert_phase", &params)?;
                if let Some(id) = whole(&answer["id"]) {
                    self.select(&[id]);
                }
            }
            "orientation" => {
                let word = value.ok_or_else(|| refused(ID, "`orientation` needs a `value`: horizontal or vertical"))?;
                let mut params = json!({"orientation": word, "page": page});
                if let (Some(map), Some(t)) = (params.as_object_mut(), self.any_selected_part()) {
                    map.insert("id".to_owned(), json!(t));
                }
                self.execute("swimlane.orientation", &params)?;
            }
            "rotate_label" => {
                let mut ids: Vec<u32> = self
                    .ui
                    .swimlane
                    .selected
                    .iter()
                    .filter(|(_, r, _)| matches!(r.as_str(), "lane" | "lanes" | "frame"))
                    .map(|(id, _, _)| *id)
                    .collect();
                if ids.is_empty() {
                    ids = self.ui.swimlane.first.map(|(frame, list)| frame.unwrap_or(list)).into_iter().collect();
                }
                self.execute("swimlane.rotate_label", &json!({"ids": ids, "page": page}))?;
            }
            "earlier" | "later" => {
                let lane = self.selected_part("lane").ok_or_else(|| refused(ID, "select a lane to move"))?;
                let (_, order) = self
                    .ui
                    .swimlane
                    .lanes
                    .iter()
                    .find(|(_, lanes)| lanes.contains(&lane))
                    .cloned()
                    .ok_or_else(|| refused(ID, "the lane is in no list of lanes"))?;
                let at = order.iter().position(|l| *l == lane).unwrap_or(0);
                let to = if op == "earlier" { at.saturating_sub(1) } else { (at + 1).min(order.len().saturating_sub(1)) };
                self.execute("swimlane.move", &json!({"lane": lane, "to": to, "page": page}))?;
            }
            other => {
                return Err(refused(ID, &format!("no operation `{other}`: insert, separator, orientation, rotate_label, earlier or later")));
            }
        }
        Ok(())
    }

    /// `window.swimlaneDo` from a map of parameters.
    pub(crate) fn swimlane_window_command(&mut self, id: &str, p: &Map<String, Value>) -> diacraft_engine::Result<()> {
        let op = p.get("op").and_then(Value::as_str).ok_or_else(|| refused(id, "missing `op`"))?;
        self.swimlane_do(op, p.get("value").and_then(Value::as_str))
    }
}
