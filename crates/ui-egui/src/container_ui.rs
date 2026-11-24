//! Containers in the window: the Insert tab's Container gallery, the Container Format tab that shows while a container is selected, the right-click row that adds a
//! shape to the container beneath it, and the light a container gives when one of its members is selected.
//!
//! **The window holds no container state of its own.** What the tab shows (the style, the heading's side, the resizing, the margin, the lock) is read from the engine with
//! `container.info` for the shapes selected, once for each revision of the drawing, page and selection ([`DiacraftApp::refresh_container_view`]); and every press is a command of
//! the engine (`container.add`, `container.style`, `container.heading`, `container.margin`, `container.fit`, `container.auto_resize`, `container.lock`, `container.disband`,
//! `container.select_contents`, `container.add_member`) sent by the window command `window.containerAdd` or `window.containerDo`, which add what only the window knows: the shapes
//! selected, the page on view and the middle of the view (an empty container is dropped there). The tiles of the galleries are drawn here, in the theme's colours, from the
//! engine's table of styles (`diacraft_engine::draw::container::GALLERY`): a frame, a strip and a mark for the heading, no picture of another application.

use std::collections::HashSet;

use diacraft_engine::EngineError;
use diacraft_engine::draw::container::{Frame, GALLERY, MARGIN_PRESETS, StyleSpec};
use egui::{CornerRadius, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::edit::Cx as Canvas;
use crate::ribbon::controls::Cx;
use crate::ribbon::spec::{Ctl, Mark};
use crate::theme::Tokens;
use crate::widgets;

/// The most shapes the tab asks the engine about at once (a selection of more is not one a container's tab is read for).
const MAX_ASKED: usize = 4_096;

/// The size of a tile of the gallery, the room between two, and the tiles in a row.
const TILE: egui::Vec2 = egui::vec2(60.0, 42.0);
const GAP: f32 = 6.0;
const COLUMNS: usize = 4;

/// What the engine says of one container selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Facts {
    pub id: u32,
    pub style: Option<String>,
    pub heading: String,
    pub resize: String,
    /// In paper inches.
    pub margin: f64,
    pub locked: bool,
    pub members: Vec<u32>,
}

/// What the Container Format tab shows of the selection.
#[derive(Debug, Default)]
pub struct ContainerView {
    /// The drawing's revision, the page and the selection this was read for.
    key: Option<(u64, u32, Vec<u32>)>,
    /// The containers among the shapes selected.
    pub(crate) containers: Vec<Facts>,
    /// For a shape selected that is no container, the containers that hold it (they are lit).
    pub(crate) held_by: Vec<(u32, Vec<u32>)>,
}

fn ids_in(list: &Value) -> Vec<u32> {
    list.as_array().map(|a| a.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect()).unwrap_or_default()
}

fn refused(cmd: &str, msg: &str) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.to_owned() }
}

impl DiacraftApp {
    /// Reads what the Container Format tab shows, if the drawing, the page or the selection has changed since it was read. Called before the ribbon is drawn.
    pub(crate) fn refresh_container_view(&mut self) {
        let Some(page) = self.current_page() else {
            self.ui.container = ContainerView::default();
            return;
        };
        let selection: Vec<u32> = self.selection().iter().copied().take(MAX_ASKED).collect();
        let key = (self.session.revision(), page, selection.clone());
        if self.ui.container.key.as_ref() == Some(&key) {
            return;
        }
        let mut view = ContainerView { key: Some(key), ..ContainerView::default() };
        if !selection.is_empty()
            && self.session.document().is_some()
            && let Ok(answer) = self.session.execute("container.info", &json!({"ids": selection, "page": page}))
        {
            for c in answer["containers"].as_array().into_iter().flatten() {
                view.containers.push(Facts {
                    id: c["id"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    style: c["style"].as_str().map(str::to_owned),
                    heading: c["heading"].as_str().unwrap_or("top").to_owned(),
                    resize: c["resize"].as_str().unwrap_or("expand").to_owned(),
                    margin: c["margin"].as_f64().unwrap_or(0.0),
                    locked: c["locked"].as_bool().unwrap_or(false),
                    members: ids_in(&c["members"]),
                });
            }
            for h in answer["held_by"].as_array().into_iter().flatten() {
                if let Some(id) = h["id"].as_u64().and_then(|n| u32::try_from(n).ok()) {
                    view.held_by.push((id, ids_in(&h["containers"])));
                }
            }
        }
        self.ui.container = view;
    }

    /// What the window knows of containers, for the window state: the containers among the shapes selected, and for a member selected the containers that hold it.
    pub(crate) fn container_state(&self) -> Value {
        let held: Vec<Value> = self.ui.container.held_by.iter().map(|(id, holders)| json!({"id": id, "containers": holders})).collect();
        json!({"selected": self.ui.container.containers.iter().map(|c| c.id).collect::<Vec<_>>(), "heldBy": held})
    }

    /// Whether a container is selected: what makes the Container Format tab show.
    pub(crate) fn container_selected(&self) -> bool {
        !self.ui.container.containers.is_empty()
    }

    /// `window.containerAdd`: a container of the gallery's `style` (the default style when `None`) round the shapes selected, or an empty one in the middle of the view; the
    /// container is selected, so that its tab shows.
    pub(crate) fn container_add(&mut self, style: Option<&str>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.containerAdd";
        let page = self.current_page().ok_or_else(|| refused(ID, "no document open"))?;
        let ids: Vec<u32> = self.selection().to_vec();
        let mut params = json!({"page": page});
        if let (Some(map), Some(style)) = (params.as_object_mut(), style) {
            map.insert("style".to_owned(), json!(style));
        }
        if ids.is_empty() {
            let mut middle = (0.0, 0.0);
            self.with_view(|view, _| middle = view.center);
            if let Some(map) = params.as_object_mut() {
                map.insert("x".to_owned(), json!(middle.0));
                map.insert("y".to_owned(), json!(middle.1));
            }
        } else if let Some(map) = params.as_object_mut() {
            map.insert("ids".to_owned(), json!(ids));
        }
        let answer = self.execute("container.add", &params)?;
        if let Some(id) = answer["id"].as_u64().and_then(|n| u32::try_from(n).ok()) {
            self.select(&[id]);
        }
        Ok(())
    }

    /// `window.containerDo`: what the Container Format tab and the right-click row do, to the containers selected (or the shapes, for `add_member`).
    pub(crate) fn container_do(&mut self, op: &str, value: Option<&str>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.containerDo";
        let page = self.current_page().ok_or_else(|| refused(ID, "no document open"))?;
        let shapes: Vec<u32> = self.selection().to_vec();
        let containers: Vec<u32> = self.ui.container.containers.iter().map(|c| c.id).collect();
        let named = |what: &str| value.ok_or_else(|| refused(ID, &format!("`{op}` needs a `value`: {what}")));
        if op != "add_member" && containers.is_empty() {
            return Err(refused(ID, "no container is selected"));
        }
        match op {
            "style" => self.execute("container.style", &json!({"ids": containers, "style": named("a style of the gallery")?, "page": page}))?,
            "heading" if value == Some("rotate") => self.execute("container.heading", &json!({"ids": containers, "rotate": true, "page": page}))?,
            "heading" => {
                self.execute("container.heading", &json!({"ids": containers, "side": named("top, right, bottom, left or rotate")?, "page": page}))?
            }
            "margin" => {
                let margin = match named("a margin: none, narrow, medium, wide or inches")?.parse::<f64>() {
                    Ok(inches) => json!(inches),
                    Err(_) => json!(value),
                };
                self.execute("container.margin", &json!({"ids": containers, "margin": margin, "page": page}))?
            }
            "fit" => self.execute("container.fit", &json!({"ids": containers, "page": page}))?,
            "auto_resize" => {
                self.execute("container.auto_resize", &json!({"ids": containers, "mode": named("none, expand or fit")?, "page": page}))?
            }
            "lock" => self.execute("container.lock", &json!({"ids": containers, "page": page}))?,
            "disband" => {
                // The shapes the frames held are left selected.
                let held: Vec<u32> = self.ui.container.containers.iter().flat_map(|c| c.members.iter().copied()).collect();
                let answer = self.execute("container.disband", &json!({"ids": containers, "page": page}))?;
                self.select(&held);
                answer
            }
            "select_contents" => {
                let answer = self.execute("container.select_contents", &json!({"ids": containers, "page": page}))?;
                self.select(&ids_in(&answer["ids"]));
                answer
            }
            "add_member" => {
                if shapes.is_empty() {
                    return Err(refused(ID, "nothing is selected"));
                }
                self.execute("container.add_member", &json!({"ids": shapes, "page": page}))?
            }
            other => {
                return Err(refused(
                    ID,
                    &format!("no operation `{other}`: style, heading, margin, fit, auto_resize, lock, disband, select_contents or add_member"),
                ));
            }
        };
        Ok(())
    }

    /// `window.containerDo` and `window.containerAdd` from a map of parameters.
    pub(crate) fn container_window_command(&mut self, id: &str, p: &Map<String, Value>) -> diacraft_engine::Result<()> {
        let text = |key: &str| p.get(key).and_then(Value::as_str);
        if id == "window.containerAdd" {
            return self.container_add(text("style"));
        }
        let op = text("op").ok_or_else(|| refused(id, "missing `op`"))?;
        self.container_do(op, text("value"))
    }

    /// Whether every container selected is locked (what Lock Container shows pressed).
    pub(crate) fn container_locked(&self) -> bool {
        let c = &self.ui.container.containers;
        !c.is_empty() && c.iter().all(|f| f.locked)
    }

    /// Whether the row of a menu that names a setting of the containers selected is the setting they all have.
    pub(crate) fn container_mark_is_current(&self, mark: Mark) -> bool {
        let c = &self.ui.container.containers;
        if c.is_empty() {
            return false;
        }
        match mark {
            Mark::ContainerResize(word) => c.iter().all(|f| f.resize == word),
            Mark::ContainerHeading(word) => c.iter().all(|f| f.heading == word),
            Mark::ContainerMargin(word) => {
                MARGIN_PRESETS.iter().find(|(name, _)| *name == word).is_some_and(|(_, inches)| c.iter().all(|f| (f.margin - inches).abs() < 1.0e-6))
            }
            _ => false,
        }
    }

    /// The light a container gives while one of its members is selected: its frame in the edit colour, over the picture.
    pub(crate) fn paint_container_glow(&self, painter: &Painter, t: &Tokens, cx: &Canvas) {
        if self.ui.edit.scene().is_none_or(|s| s.page != cx.page) {
            return;
        }
        let mut lit: HashSet<u32> = HashSet::new();
        for (_, holders) in &self.ui.container.held_by {
            for c in holders {
                if !lit.insert(*c) {
                    continue;
                }
                if let Some(corners) = self.scene_corners(*c) {
                    let corners: Vec<Pos2> = corners.iter().map(|p| cx.screen_at(*p)).collect();
                    painter.add(Shape::closed_line(corners, Stroke::new(3.0, t.edit_line)));
                }
            }
        }
    }
}

// ---- the tiles ---------------------------------------------------------------------------------------------------------------------------------

/// Draws the tile of `style` in `r`: the frame, the strip with its divider and the mark where the heading goes, in the theme's ink. The frame the person has now is outlined in the
/// accent.
fn paint_style(painter: &Painter, t: &Tokens, r: Rect, style: &StyleSpec, current: bool, enabled: bool) {
    let ink = if enabled { t.icon_ink } else { t.icon_ink_disabled };
    painter.rect(
        r,
        CornerRadius::same(3),
        t.input_fill,
        Stroke::new(if current { 2.0 } else { 1.0 }, if current { t.accent } else { t.input_border }),
        StrokeKind::Inside,
    );
    let inner = r.shrink2(vec2(7.0, 6.0));
    if style.tint {
        painter.rect_filled(inner, 0.0, t.edit_fill);
    }
    let weight = if style.frame == Frame::Heavy { 2.2 } else { 1.0 };
    let stroke = Stroke::new(weight, ink);
    let corners = [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom()];
    match style.frame {
        Frame::Plain | Frame::Heavy => {
            painter.rect_stroke(inner, 0.0, stroke, StrokeKind::Inside);
        }
        Frame::Dashed => {
            let mut line = corners.to_vec();
            line.push(corners[0]);
            painter.extend(Shape::dashed_line(&line, Stroke::new(1.0, ink), 3.0, 2.0));
        }
        Frame::Brackets => {
            let arm = 7.0;
            for (corner, dx, dy) in [(corners[0], 1.0, 1.0), (corners[1], -1.0, 1.0), (corners[2], -1.0, -1.0), (corners[3], 1.0, -1.0)] {
                painter.line_segment([corner + vec2(dx * arm, 0.0), corner], stroke);
                painter.line_segment([corner, corner + vec2(0.0, dy * arm)], stroke);
            }
        }
        // The gallery has no style with only an edge (that look is the phase's of a cross-functional flowchart).
        Frame::Open | Frame::Edge => {}
    }
    let band = 8.0;
    if style.strip {
        let y = inner.top() + band;
        painter.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0, ink));
    }
    // The heading: a short bar in the strip's place, at the left or in the middle.
    let bar = vec2(16.0, 2.5);
    let at_y = inner.top() + band / 2.0 - bar.y / 2.0 + if style.strip { 0.0 } else { 1.5 };
    let x = if style.centred { inner.center().x - bar.x / 2.0 } else { inner.left() + 4.0 };
    painter.rect_filled(Rect::from_min_size(pos2(x, at_y), bar), 1.0, ink);
}

/// The gallery's tiles in a menu, under a control: a press on one sends the control's command with its style. The Insert tab's control adds a container (`window.containerAdd`); the
/// Container Format tab's changes the style of the containers selected (`window.containerDo`), and marks the one they have.
pub(crate) fn style_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl) {
    let adds = ctl.command == Some("window.containerAdd");
    let enabled = ctl.command.is_some_and(|c| app.command_enabled(c).is_ok());
    let current: Option<String> = if adds {
        None
    } else {
        app.ui.container.containers.first().filter(|_| app.ui.container.containers.len() == 1).and_then(|f| f.style.clone())
    };
    for row in GALLERY.chunks(COLUMNS) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for style in row {
                let (r, resp) = ui.allocate_exact_size(TILE, if enabled { Sense::click() } else { Sense::hover() });
                let marked = current.as_deref() == Some(style.id);
                widgets::name_button(&resp, enabled, style.name);
                paint_style(ui.painter(), &cx.t, r, style, marked || resp.hovered(), enabled);
                app.note(format!("menu.container.{}", style.id), r, enabled);
                let resp = resp.on_hover_text(style.name);
                if resp.clicked() && enabled {
                    let params = if adds { json!({"style": style.id}) } else { json!({"op": "style", "value": style.id}) };
                    if let Some(command) = ctl.command {
                        app.run_ribbon_params(ui.ctx(), command, &params);
                    }
                    ui.close();
                }
            }
        });
        ui.add_space(GAP / 2.0);
    }
}

/// The width the gallery's menu needs, in points.
pub(crate) const fn menu_width() -> f32 {
    COLUMNS as f32 * TILE.x + (COLUMNS as f32 - 1.0) * GAP + 16.0
}
