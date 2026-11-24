//! The Organization Chart in the window: the Org Chart tab that shows while the drawing holds an org chart, and the window command that sends
//! its controls to the engine.
//!
//! **The window holds no chart state of its own.** What the tab shows is read from the engine with `orgchart.info` for the page on view and the shapes selected, once for each revision of
//! the drawing, page and selection ([`DiacraftApp::refresh_org_view`]); and every press is a command of the engine (`orgchart.layout`, `orgchart.relayout`, `orgchart.arrange`,
//! `orgchart.picture_set`, `orgchart.picture_delete`, `orgchart.picture_hide`) sent by the window command `window.orgChartDo`, which adds what only the window knows: the page on view and the
//! position selected, and the chooser a picture file is picked with (the one Insert, Pictures uses, `insert_picture.rs`).

use std::path::Path;

use diacraft_engine::EngineError;
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;

/// The most shapes the tab asks the engine about at once.
const MAX_ASKED: usize = 1_024;

/// What the engine says of the shape selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Facts {
    pub id: u32,
    pub position: bool,
    pub arrangement: String,
    pub has_picture: bool,
    pub hidden_picture: bool,
}

/// What the Org Chart tab shows.
#[derive(Debug, Default)]
pub struct OrgView {
    /// The drawing's revision, the page and the selection this was read for.
    key: Option<(u64, u32, Vec<u32>)>,
    /// Whether the drawing holds an org chart: what makes the tab show.
    pub(crate) present: bool,
    /// The layout the page holds (one of the six of `orgchart.layout`), if it holds one.
    pub(crate) layout: Option<String>,
    /// The shapes selected that are positions.
    pub(crate) selected: Vec<Facts>,
}

fn refused(cmd: &str, msg: &str) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.to_owned() }
}

impl DiacraftApp {
    /// Reads what the Org Chart tab shows, if the drawing, the page or the selection has changed since it was read. Called before the ribbon is drawn.
    pub(crate) fn refresh_org_view(&mut self) {
        let Some(page) = self.current_page() else {
            self.ui.org = OrgView::default();
            return;
        };
        let selection: Vec<u32> = self.selection().iter().copied().take(MAX_ASKED).collect();
        let key = (self.session.revision(), page, selection.clone());
        if self.ui.org.key.as_ref() == Some(&key) {
            return;
        }
        let mut view = OrgView { key: Some(key), ..OrgView::default() };
        if self.session.document().is_some() {
            // `quick` stops looking for positions at the first page that holds one: the tab only needs to know there is a chart.
            match self.session.execute("orgchart.info", &json!({"page": page, "ids": selection, "quick": true})) {
                Ok(answer) => {
                    view.present = answer["document_positions"].as_u64().unwrap_or(0) > 0;
                    view.layout = answer["layout"].as_str().map(str::to_owned);
                    for s in answer["selected"].as_array().into_iter().flatten() {
                        if s["position"].as_bool() != Some(true) {
                            continue;
                        }
                        view.selected.push(Facts {
                            id: s["id"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                            position: true,
                            arrangement: s["arrangement"].as_str().unwrap_or("inherit").to_owned(),
                            has_picture: s["picture"]["has"].as_bool().unwrap_or(false),
                            hidden_picture: s["picture"]["hidden"].as_bool().unwrap_or(false),
                        });
                    }
                }
                // A page too large to read is no chart the tab can serve; the engine says why when a control is pressed.
                Err(_) => view.present = false,
            }
        }
        self.ui.org = view;
    }

    /// What the window knows of the org chart, for the window state: whether the tab shows, the layout the page holds, and the positions selected.
    pub(crate) fn org_state(&self) -> Value {
        let selected: Vec<Value> = self
            .ui
            .org
            .selected
            .iter()
            .map(|f| json!({"id": f.id, "arrangement": f.arrangement, "picture": {"has": f.has_picture, "hidden": f.hidden_picture}}))
            .collect();
        json!({"present": self.ui.org.present, "layout": self.ui.org.layout, "selected": selected})
    }

    /// Whether the drawing holds an org chart: what makes the Org Chart tab show.
    pub(crate) fn org_chart_present(&self) -> bool {
        self.ui.org.present
    }

    /// The position selected, when exactly one is.
    fn single_position(&self, id: &str) -> diacraft_engine::Result<u32> {
        match self.ui.org.selected.as_slice() {
            [one] => Ok(one.id),
            [] => Err(refused(id, "select a position of the org chart first")),
            _ => Err(refused(id, "select one position")),
        }
    }

    /// Whether the row of a menu that names a layout or an arrangement is the one the page holds, or the position selected has.
    pub(crate) fn org_mark_is_current(&self, mark: crate::ribbon::spec::Mark) -> bool {
        use crate::ribbon::spec::Mark;
        match mark {
            Mark::OrgLayout(word) => self.ui.org.layout.as_deref() == Some(word),
            Mark::OrgArrangement(word) => matches!(self.ui.org.selected.as_slice(), [one] if one.arrangement == word),
            _ => false,
        }
    }

    /// Whether the picture of the position selected is hidden: what Hide Picture shows pressed.
    pub(crate) fn org_picture_hidden(&self) -> bool {
        matches!(self.ui.org.selected.as_slice(), [one] if one.hidden_picture)
    }

    /// `window.orgChartDo`: what the Org Chart tab does, to the page on view and the position selected.
    pub(crate) fn org_do(&mut self, op: &str, value: Option<&str>, path: Option<&str>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.orgChartDo";
        let page = self.current_page().ok_or_else(|| refused(ID, "no document open"))?;
        let named = |what: &str| value.ok_or_else(|| refused(ID, &format!("`{op}` needs a `value`: {what}")));
        match op {
            "layout" => {
                self.execute("orgchart.layout", &json!({"layout": named("one of the layouts of orgchart.layout")?, "page": page}))?;
            }
            "relayout" => {
                self.execute("orgchart.relayout", &json!({"page": page}))?;
            }
            "arrange" => {
                let id = self.single_position(ID)?;
                self.execute(
                    "orgchart.arrange",
                    &json!({"id": id, "arrangement": named("horizontal, vertical_right, vertical_left or inherit")?, "page": page}),
                )?;
            }
            "picture" => {
                let id = self.single_position(ID)?;
                return self.org_picture(id, path);
            }
            "picture_delete" => {
                let id = self.single_position(ID)?;
                self.execute("orgchart.picture_delete", &json!({"id": id, "page": page}))?;
            }
            "picture_hide" => {
                let id = self.single_position(ID)?;
                let hidden = self.ui.org.selected.iter().find(|f| f.id == id).is_some_and(|f| f.hidden_picture);
                self.execute("orgchart.picture_hide", &json!({"id": id, "hidden": !hidden, "page": page}))?;
            }
            other => {
                return Err(refused(ID, &format!("no operation `{other}`: layout, relayout, arrange, picture, picture_delete or picture_hide")));
            }
        }
        Ok(())
    }

    /// `window.orgChartDo` from a map of parameters.
    pub(crate) fn org_window_command(&mut self, p: &Map<String, Value>) -> diacraft_engine::Result<()> {
        let text = |key: &str| p.get(key).and_then(Value::as_str);
        let op = text("op")
            .ok_or_else(|| refused("window.orgChartDo", "missing `op`: layout, relayout, arrange, picture, picture_delete or picture_hide"))?;
        self.org_do(op, text("value"), text("path"))
    }

    /// Insert Picture and Change Picture on the position `id`: the file at `path` with one, else the chooser (shown by the next frame), whose answer goes to `orgchart.picture_set`.
    fn org_picture(&mut self, id: u32, path: Option<&str>) -> diacraft_engine::Result<()> {
        if let Some(path) = path {
            self.org_picture_file(id, Path::new(path))?;
            return Ok(());
        }
        self.command_insert_picture_for(Some(id), None)?;
        Ok(())
    }

    /// `orgchart.picture_set` of the file at `path` on position `id`; a format that is kept and drawn as a placeholder says so in the status bar.
    pub(crate) fn org_picture_file(&mut self, id: u32, path: &Path) -> diacraft_engine::Result<Value> {
        const ID: &str = "window.orgChartDo";
        let text = path.to_str().ok_or_else(|| refused(ID, "the name of the file is not text"))?;
        self.commit_pending_text();
        let mut params = json!({ "id": id, "path": text });
        if let (Some(page), Some(map)) = (self.current_page(), params.as_object_mut()) {
            map.insert("page".to_owned(), json!(page));
        }
        let answer = self.execute("orgchart.picture_set", &params)?;
        self.ui.insert.note = answer.get("note").and_then(Value::as_str).map(|n| {
            let mut chars = n.chars();
            chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str() + ".")
        });
        Ok(answer)
    }
}
