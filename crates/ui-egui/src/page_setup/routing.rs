//! The Layout and Routing tab of the Page Setup dialog (the connector routing design, "The UI").
//!
//! The tab **reads `page.routing`** when the dialog opens and works on the dialog's **draft** like the other tabs: nothing is sent until OK, which adds to the commands the other tabs
//! send **one `page.set_routing` and one `connector.line_jumps`, each holding only what the person changed** (a cell nobody touched is never written), all in the one
//! `command.batch` labelled Page setup, so that they are one undo step; Cancel and Escape send nothing.
//!
//! * **Routing:** Style and Direction (Direction is greyed for the styles that take none: Right Angle, Straight, Network and Center to Center), Separate, Overlap and Appearance;
//!   a new Style asks the engine to route again the connectors that follow the page, and nothing else on this tab routes anything.
//! * **Line jumps:** Add line jumps to, Line jump style, and Vertical size and Horizontal size, each a slider and a box from 0 to 10 (`LineJumpFactorY` and `LineJumpFactorX`).
//! * **Other:** Move other shapes away on drop and Enable connector splitting write `PlowCode` and `PageShapeSplit`; DiaCraft acts on neither yet, and the group says so in a line
//!   under the boxes and in each box's tip.
//! * **Spacing...** opens the Layout and Routing Spacing dialog (`routing_spacing.rs`), whose OK puts four groups of two lengths in this draft.
//! * **The preview** (`routing_preview.rs`) is routed by `diacraft-route` with the draft's choices.
//!
//! **Lengths are lengths on paper** (the unit `page.set_routing` takes), shown in inches, or in millimetres on a page whose scale is given in a metric unit.
//! What the engine reads for a cell a page does not hold is the root's value *in drawing units*, so on a scaled page the tab shows that value as the paper length it is (an absent
//! `LineToNodeX` of 0.125 drawing inch at a scale of 10 is 0.0125 inch), while the Spacing dialog's Defaults are the root's values *on paper*: the two differ there, and the
//! window shows both as they are.

use diacraft_engine::draw::RouteStyle;
use diacraft_engine::draw::length_unit;
use diacraft_units::Unit;
use egui::{Align, Layout, RichText, Ui, vec2};
use serde_json::{Map, Value, json};

use super::routing_preview::{Inputs, Look, Preview, paint};
use super::routing_spacing::Spacing;
use super::{Dialog, heading, label};
use crate::measure::{number_text, parse_length};
use crate::theme::{self, Tokens, size};

pub(super) type Seen = Vec<(String, egui::Rect, bool)>;

/// How many groups of two lengths the Spacing dialog has.
pub(super) const GROUPS: usize = 4;

/// The keys of the four groups in `page.routing` and `page.set_routing`, and their captions in the Spacing dialog.
pub(super) const GROUP_KEYS: [&str; GROUPS] = ["between_shapes", "shape_size", "connector_to_connector", "connector_to_shape"];
pub(super) const GROUP_NAMES: [&str; GROUPS] = ["Space between shapes", "Average shape size", "Connector to connector", "Connector to shape"];

/// The root style's values, in the order of the groups: 3/8, 1/4, 1/8 and 1/8 inch, or 7.5, 5, 2.5 and 2.5 mm.
const ROOT_INCHES: [f64; GROUPS] = [0.375, 0.25, 0.125, 0.125];
const ROOT_MILLIMETRES: [f64; GROUPS] = [7.5, 5.0, 2.5, 2.5];

/// The widths of the tab's columns and parts, in points.
const LABEL_W: f32 = 112.0;
const ROW_H: f32 = 24.0;
const MIN_CONTROL: f32 = 96.0;
const MAX_CONTROL: f32 = 150.0;
/// The tallest an open list is: tall enough for the nine rows of the jump styles, so that none is rolled out of reach.
const LIST_H: f32 = 300.0;
const BOX_W: f32 = 58.0;
const COLUMN_W: f32 = LABEL_W + 8.0 + MAX_CONTROL;
const COLUMN_GAP: f32 = 14.0;
/// The width from which the tab is two columns (the groups at the left, the preview and the Other group at the right).
const WIDE_MIN: f32 = COLUMN_W + COLUMN_GAP + 190.0;
const PREVIEW_MAX_W: f32 = 300.0;

/// The most a jump's size is (`LineJumpFactorX` and `Y`: 0 to 10).
const MOST_SIZE: f64 = 10.0;

/// The tip of the Direction list while it is greyed: which styles take one.
const DIRECTION_TIP: &str = "A direction is for the Organization Chart, Flowchart, Tree and Simple styles.";

/// The words under the Other group and in the tip of each box: DiaCraft keeps the two cells and does nothing with them yet.
const OTHER_NOTE: &str = "DiaCraft does not act on these two settings yet; it keeps them with the page.";
const PLOW_TIP: &str = "Kept as the page's setting. DiaCraft does not move shapes aside when one is dropped, moved or resized, so this box changes nothing on the page yet.";
const SPLIT_TIP: &str =
    "Kept as the page's setting. DiaCraft does not split a connector when a shape is dropped on it, so this box changes nothing on the page yet.";

/// A row of a list: the word the commands take, the words the list shows and, for the jump lists, the cell's code.
#[derive(Clone, Copy)]
struct Row {
    word: &'static str,
    label: &'static str,
    code: u32,
}

const fn row(word: &'static str, label: &'static str) -> Row {
    Row { word, label, code: 0 }
}

const fn coded(word: &'static str, label: &'static str, code: u32) -> Row {
    Row { word, label, code }
}

const STYLES: [Row; 8] = [
    row("right_angle", "Right Angle"),
    row("straight", "Straight"),
    row("org_chart", "Organization Chart"),
    row("flowchart", "Flowchart"),
    row("tree", "Tree"),
    row("network", "Network"),
    row("centre_to_centre", "Center to Center"),
    row("simple", "Simple"),
];
const FLOWS: [Row; 4] = [
    row("top_to_bottom", "Top to Bottom"),
    row("left_to_right", "Left to Right"),
    row("bottom_to_top", "Bottom to Top"),
    row("right_to_left", "Right to Left"),
];
const SIMPLE_WAYS: [Row; 6] = [
    row("top_to_bottom", "Top to Bottom"),
    row("left_to_right", "Left to Right"),
    row("bottom_to_top", "Bottom to Top"),
    row("right_to_left", "Right to Left"),
    row("level_first", "Level First"),
    row("upright_first", "Upright First"),
];
const SEPARATE: [Row; 4] =
    [row("unrelated", "Unrelated lines"), row("all", "All lines"), row("none", "No lines"), row("by_style", "Using routing style")];
const OVERLAP: [Row; 4] = [row("related", "Related lines"), row("all", "All lines"), row("none", "No lines"), row("by_style", "Using routing style")];
const APPEARANCE: [Row; 2] = [row("straight", "Straight"), row("curved", "Curved")];
const JUMP_TO: [Row; 6] = [
    coded("0", "None", 0),
    coded("1", "Horizontal Lines", 1),
    coded("2", "Vertical Lines", 2),
    coded("3", "Last Routed Line", 3),
    coded("4", "Last Displayed Line", 4),
    coded("5", "First Displayed Line", 5),
];
const JUMP_STYLES: [Row; 9] = [
    coded("arc", "Arc", 0),
    coded("gap", "Gap", 2),
    coded("square", "Square", 3),
    coded("sides2", "2 Sides", 4),
    coded("sides3", "3 Sides", 5),
    coded("sides4", "4 Sides", 6),
    coded("sides5", "5 Sides", 7),
    coded("sides6", "6 Sides", 8),
    coded("sides7", "7 Sides", 9),
];

/// The row of `rows` that says `word`.
fn find(rows: &'static [Row], word: &str) -> Option<&'static Row> {
    rows.iter().find(|r| r.word == word)
}

/// The word of `rows` that `word` is, or `otherwise` when the engine says one the list does not have.
fn known(rows: &'static [Row], word: &str, otherwise: &'static str) -> &'static str {
    find(rows, word).map_or(otherwise, |r| r.word)
}

/// The directions a style takes: none for Right Angle, Straight, Network and Center to Center.
fn directions(style: &str) -> &'static [Row] {
    match style {
        "simple" => &SIMPLE_WAYS,
        "org_chart" | "flowchart" | "tree" => &FLOWS,
        _ => &[],
    }
}

/// What `page.routing` said.
#[derive(Clone, Debug, PartialEq)]
struct Base {
    style: String,
    direction: Option<String>,
    separate: String,
    overlap: String,
    appearance: String,
    jump_code: u32,
    jump_style: u32,
    factor_x: f64,
    factor_y: f64,
    plow: bool,
    split: bool,
    /// The four groups in paper inches.
    spacing: [(f64, f64); GROUPS],
}

impl Base {
    fn of(answer: &Value) -> Option<Base> {
        let text = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_owned);
        let number = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|n| n.is_finite());
        let whole = |v: Option<&Value>| v.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        let pair = |key: &str| -> Option<(f64, f64)> {
            let p = answer.get(key)?;
            Some((number(p.get("x"))?, number(p.get("y"))?))
        };
        let [first, second, third, fourth] = GROUP_KEYS;
        let jumps = answer.get("line_jumps")?;
        Some(Base {
            style: text("style")?,
            direction: text("direction"),
            separate: text("separate")?,
            overlap: text("overlap")?,
            appearance: text("appearance")?,
            jump_code: whole(jumps.get("code"))?,
            jump_style: whole(jumps.get("style"))?,
            factor_x: number(jumps.get("factor_x"))?,
            factor_y: number(jumps.get("factor_y"))?,
            plow: answer.get("plow")?.as_bool()?,
            split: answer.get("split")?.as_bool()?,
            spacing: [pair(first)?, pair(second)?, pair(third)?, pair(fourth)?],
        })
    }

    /// The jump list's choice a cell code is: 6 and over give no jump, as 0.
    fn jump_to(&self) -> u32 {
        if self.jump_code <= 5 { self.jump_code } else { 0 }
    }

    /// The jump style list's choice a cell code is: 1 is an arc as 0 is, and a code the list has not is an arc.
    fn jump_style(&self) -> u32 {
        if (2..=9).contains(&self.jump_style) { self.jump_style } else { 0 }
    }
}

/// The unit the tab shows lengths in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shown {
    Inches,
    Millimetres,
}

impl Shown {
    /// Millimetres where the page's scale is given in a metric unit on paper (a drawing in millimetres), inches otherwise.
    fn of(paper_unit: &str) -> Shown {
        match length_unit(paper_unit).map(Unit::code) {
            Some("MM" | "CM" | "M" | "KM") => Shown::Millimetres,
            _ => Shown::Inches,
        }
    }

    fn unit(self) -> Unit {
        match self {
            Shown::Inches => Unit::Inch,
            Shown::Millimetres => Unit::Millimetre,
        }
    }

    pub(super) fn word(self) -> &'static str {
        match self {
            Shown::Inches => "in",
            Shown::Millimetres => "mm",
        }
    }

    /// A length on paper, in inches, as a box shows it.
    pub(super) fn text(self, inches: f64) -> String {
        number_text(self.unit().from_internal(inches))
    }

    /// The root's value of group `group`, as a box shows it.
    pub(super) fn root(self, group: usize) -> String {
        let table = if self == Shown::Inches { &ROOT_INCHES } else { &ROOT_MILLIMETRES };
        number_text(table.get(group).copied().unwrap_or(0.0))
    }
}

/// The Layout and Routing tab's draft, over what the page said when the dialog opened.
pub(super) struct Routing {
    base: Base,
    pub(super) shown: Shown,
    style: &'static str,
    direction: &'static str,
    separate: &'static str,
    overlap: &'static str,
    appearance: &'static str,
    jump_to: &'static str,
    jump_style: &'static str,
    /// The two size boxes, as typed: Vertical (`LineJumpFactorY`) and Horizontal (`LineJumpFactorX`), and what each held when the dialog opened.
    vertical: String,
    horizontal: String,
    /// The last number each box held that was one: the slider shows it while the text is not.
    sliders: (f64, f64),
    sizes_at_open: (String, String),
    plow: bool,
    split: bool,
    /// The four groups as typed, and as they were when the dialog opened (a box that still says what it said is not looked at again).
    pub(super) spacing: [(String, String); GROUPS],
    spacing_at_open: [(String, String); GROUPS],
    /// The Spacing dialog, while it is open.
    pub(super) dialog: Option<Spacing>,
    preview: Preview,
}

impl Routing {
    /// What page `page` says of its routing, as a draft; `paper_unit` is the unit of the page's scale on paper. `None` when the engine does not answer.
    pub(super) fn read(session: &mut diacraft_engine::Session, page: u32, paper_unit: &str) -> Option<Routing> {
        let answer = session.execute("page.routing", &json!({"page": page})).ok()?;
        Some(Routing::from_base(Base::of(&answer)?, Shown::of(paper_unit)))
    }

    /// The draft of a page that says `base`. **A word the lists do not have is the first row of its list, in the draft and in what it is compared with**, so that a list the person did
    /// not touch never sends a word of its own.
    fn from_base(mut base: Base, shown: Shown) -> Routing {
        base.style = known(&STYLES, &base.style, "right_angle").to_owned();
        base.separate = known(&SEPARATE, &base.separate, "unrelated").to_owned();
        base.overlap = known(&OVERLAP, &base.overlap, "by_style").to_owned();
        base.appearance = known(&APPEARANCE, &base.appearance, "straight").to_owned();
        let ways = directions(&base.style);
        let direction = base.direction.as_deref().and_then(|d| find(ways, d)).map_or("top_to_bottom", |r| r.word);
        base.direction = (!ways.is_empty()).then(|| direction.to_owned());
        let spacing = base.spacing.map(|(x, y)| (shown.text(x), shown.text(y)));
        let (vertical, horizontal) = (number_text(base.factor_y), number_text(base.factor_x));
        Routing {
            style: known(&STYLES, &base.style, "right_angle"),
            direction,
            separate: known(&SEPARATE, &base.separate, "unrelated"),
            overlap: known(&OVERLAP, &base.overlap, "by_style"),
            appearance: known(&APPEARANCE, &base.appearance, "straight"),
            jump_to: JUMP_TO.get(usize::try_from(base.jump_to()).unwrap_or(0)).map_or("0", |r| r.word),
            jump_style: JUMP_STYLES.iter().find(|r| r.code == base.jump_style()).map_or("arc", |r| r.word),
            sliders: (base.factor_y, base.factor_x),
            sizes_at_open: (vertical.clone(), horizontal.clone()),
            vertical,
            horizontal,
            plow: base.plow,
            split: base.split,
            spacing_at_open: spacing.clone(),
            spacing,
            dialog: None,
            preview: Preview::default(),
            shown,
            base,
        }
    }

    /// The direction the draft's style has, if it takes one.
    fn direction_now(&self) -> Option<&'static str> {
        (!directions(self.style).is_empty()).then_some(self.direction)
    }

    /// After the style was changed: a direction the new style does not take starts over from the top.
    fn restyled(&mut self) {
        if !directions(self.style).iter().any(|r| r.word == self.direction) {
            self.direction = "top_to_bottom";
        }
    }

    /// The style the draft says, as the engine reads it.
    fn route_style(&self) -> RouteStyle {
        RouteStyle::choose(Some(self.style), self.direction_now(), RouteStyle::RightAngle).unwrap_or(RouteStyle::RightAngle)
    }

    /// A length typed in a box, in inches, if it is one: more than 0.
    pub(super) fn length(&self, text: &str) -> Option<f64> {
        parse_length(text, self.shown.unit()).ok().filter(|n| n.is_finite() && *n > 0.0)
    }

    /// What the preview shows: the draft's choices, and its lengths (the page's own for a box that does not say a length now).
    fn inputs(&self) -> Inputs {
        let pair = |group: usize| -> (f64, f64) {
            let held = self.base.spacing.get(group).copied().unwrap_or((0.125, 0.125));
            let typed = self.spacing.get(group);
            (typed.and_then(|(x, _)| self.length(x)).unwrap_or(held.0), typed.and_then(|(_, y)| self.length(y)).unwrap_or(held.1))
        };
        let size_of = |text: &str, last: f64| jump_size(text).unwrap_or(last);
        Inputs {
            style: self.route_style(),
            curved: self.appearance == "curved",
            clearance: pair(3),
            spacing: pair(2),
            jump_to: find(&JUMP_TO, self.jump_to).map_or(0, |r| r.code),
            look: Look::of_code(find(&JUMP_STYLES, self.jump_style).map_or(0, |r| r.code)),
            sizes: (size_of(&self.horizontal, self.sliders.1), size_of(&self.vertical, self.sliders.0)),
        }
    }

    /// The commands OK adds: a `page.set_routing` and a `connector.line_jumps`, each only with what the person changed. A box that is no good is a refusal with words.
    pub(super) fn commands(&self, page: u32) -> Result<Vec<(&'static str, Value)>, String> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let routing = self.routing_params()?;
        if !routing.is_empty() {
            let mut params = routing;
            params.insert("page".to_owned(), json!(page));
            out.push(("page.set_routing", Value::Object(params)));
        }
        let jumps = self.jump_params()?;
        if !jumps.is_empty() {
            let mut params = jumps;
            params.insert("page".to_owned(), json!(page));
            out.push(("connector.line_jumps", Value::Object(params)));
        }
        Ok(out)
    }

    fn routing_params(&self) -> Result<Map<String, Value>, String> {
        let mut params = Map::new();
        let now = self.direction_now();
        if self.base.style != self.style || self.base.direction.as_deref() != now {
            params.insert("style".to_owned(), json!(self.style));
            if let Some(direction) = now {
                params.insert("direction".to_owned(), json!(direction));
            }
        }
        for (key, now, was) in [
            ("separate", self.separate, self.base.separate.as_str()),
            ("overlap", self.overlap, self.base.overlap.as_str()),
            ("appearance", self.appearance, self.base.appearance.as_str()),
        ] {
            if now != was {
                params.insert(key.to_owned(), json!(now));
            }
        }
        for (key, now, was) in [("plow", self.plow, self.base.plow), ("split", self.split, self.base.split)] {
            if now != was {
                params.insert(key.to_owned(), json!(now));
            }
        }
        for (group, key) in GROUP_KEYS.iter().enumerate() {
            let mut pair = Map::new();
            for (axis, name) in ["x", "y"].into_iter().enumerate() {
                if let Some(inches) = self.changed_length(group, axis)? {
                    pair.insert(name.to_owned(), json!(inches));
                }
            }
            if !pair.is_empty() {
                params.insert((*key).to_owned(), Value::Object(pair));
            }
        }
        Ok(params)
    }

    /// The length of box `axis` (0 across, 1 up) of group `group`, in paper inches, if the person changed it: a box that says what it said is not looked at, and one that was changed and
    /// is no length more than 0 is a refusal.
    fn changed_length(&self, group: usize, axis: usize) -> Result<Option<f64>, String> {
        let (Some(typed), Some(opened), Some(held)) = (self.spacing.get(group), self.spacing_at_open.get(group), self.base.spacing.get(group)) else {
            return Ok(None);
        };
        let (text, was, base) = if axis == 0 { (&typed.0, &opened.0, held.0) } else { (&typed.1, &opened.1, held.1) };
        if text.trim() == was.trim() {
            return Ok(None);
        }
        let inches = self.length(text).ok_or_else(|| length_words(group, axis))?;
        Ok(((inches - base).abs() > 1.0e-9).then_some(inches))
    }

    fn jump_params(&self) -> Result<Map<String, Value>, String> {
        let mut params = Map::new();
        let code = find(&JUMP_TO, self.jump_to).map_or(0, |r| r.code);
        if code != self.base.jump_to() {
            params.insert("code".to_owned(), json!(code));
        }
        let style = find(&JUMP_STYLES, self.jump_style).map_or(0, |r| r.code);
        if style != self.base.jump_style() {
            params.insert("style".to_owned(), json!(style));
        }
        for (key, text, was, base) in [
            ("factor_y", &self.vertical, &self.sizes_at_open.0, self.base.factor_y),
            ("factor_x", &self.horizontal, &self.sizes_at_open.1, self.base.factor_x),
        ] {
            if text.trim() == was.trim() {
                continue;
            }
            let value = jump_size(text).ok_or_else(|| format!("Vertical size and Horizontal size are numbers from 0 to {MOST_SIZE}."))?;
            if (value - base).abs() > 1.0e-9 {
                params.insert(key.to_owned(), json!(value));
            }
        }
        Ok(params)
    }

    /// What the window reports of the tab: the draft, the Spacing dialog if it is open, and the preview.
    pub(super) fn state_json(&self) -> Value {
        let groups = |spacing: &[(String, String); GROUPS]| -> Value {
            let mut map = Map::new();
            for (key, (x, y)) in GROUP_KEYS.iter().zip(spacing) {
                map.insert((*key).to_owned(), json!([x, y]));
            }
            Value::Object(map)
        };
        json!({
            "style": self.style,
            "direction": self.direction_now(),
            "directionEnabled": !directions(self.style).is_empty(),
            "separate": self.separate,
            "overlap": self.overlap,
            "appearance": self.appearance,
            "jumps": {"to": self.jump_to.parse::<u32>().unwrap_or(0), "style": self.jump_style, "vertical": self.vertical, "horizontal": self.horizontal},
            "other": {"plow": self.plow, "split": self.split},
            "unit": self.shown.word(),
            "spacing": groups(&self.spacing),
            "spacingDialog": self.dialog.as_ref().map(|d| groups(&d.boxes)),
            "preview": self.preview.state_json(),
        })
    }
}

/// The words of a refusal of a box of the spacing groups.
pub(super) fn length_words(group: usize, axis: usize) -> String {
    format!(
        "{}, {}: type a length more than 0, such as {} (a number takes the unit of the box, or give one: 1 cm).",
        GROUP_NAMES.get(group).copied().unwrap_or("Spacing"),
        if axis == 0 { "Horizontal" } else { "Vertical" },
        "0.25 in"
    )
}

/// A jump size typed in a box: a number from 0 to 10.
fn jump_size(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && (0.0..=MOST_SIZE).contains(v))
}

// ---- the tab ---------------------------------------------------------------------------------------------------------------------------------

/// A list of `rows` in a combo box `width` wide: the row of `current` is shown, and the rows are named `{id}.{word}` while the list is open. Whether `current` changed, and the box's response.
fn list(
    ui: &mut Ui,
    (id, words, width): (&str, &str, f32),
    enabled: bool,
    rows: &'static [Row],
    current: &mut &'static str,
    seen: &mut Seen,
) -> (bool, egui::Response) {
    let shown = find(rows, current).map_or("", |r| r.label);
    let before = *current;
    let combo = ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(id).width(width).height(LIST_H).selected_text(RichText::new(shown).font(theme::font(size::LABEL))).show_ui(
            ui,
            |ui| {
                for r in rows {
                    let item = ui.selectable_label(before == r.word, RichText::new(r.label).font(theme::font(size::LABEL)));
                    seen.push((format!("{id}.{}", r.word), item.rect, true));
                    if item.clicked() {
                        *current = r.word;
                    }
                }
            },
        )
    });
    let response = combo.inner.response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, words));
    seen.push((id.to_owned(), response.rect, enabled));
    (*current != before, response)
}

/// A row of a group: its caption in a column of its own and then what `content` puts, given the width a control may have.
fn row_of<R>(ui: &mut Ui, t: &Tokens, words: &str, content: impl FnOnce(&mut Ui, f32) -> R) -> R {
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let control = (ui.available_width() - LABEL_W - gap).clamp(MIN_CONTROL, MAX_CONTROL);
        // (An allocated child takes the room it used, so the column's width is asked for.)
        ui.allocate_ui_with_layout(vec2(LABEL_W, ROW_H), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_size(vec2(LABEL_W, ROW_H));
            label(ui, t, words);
        });
        content(ui, control)
    })
    .inner
}

/// The Routing group: Style, Direction, Separate, Overlap and Appearance.
fn routing_group(ui: &mut Ui, t: &Tokens, r: &mut Routing, seen: &mut Seen) {
    heading(ui, t, "Routing");
    row_of(ui, t, "Style", |ui, width| {
        if list(ui, ("pagesetup.routing.style", "Style", width), true, &STYLES, &mut r.style, seen).0 {
            r.restyled();
        }
    });
    let takes = !directions(r.style).is_empty();
    row_of(ui, t, "Direction", |ui, width| {
        // A style that takes none shows nothing in the box: it is greyed.
        let mut shown = if takes { r.direction } else { "" };
        let (_, response) = list(ui, ("pagesetup.routing.direction", "Direction", width), takes, directions(r.style), &mut shown, seen);
        if !takes {
            response.on_disabled_hover_text(DIRECTION_TIP);
        }
        if takes {
            r.direction = known(directions(r.style), shown, "top_to_bottom");
        }
    });
    row_of(ui, t, "Separate", |ui, width| {
        list(ui, ("pagesetup.routing.separate", "Separate", width), true, &SEPARATE, &mut r.separate, seen);
    });
    row_of(ui, t, "Overlap", |ui, width| {
        list(ui, ("pagesetup.routing.overlap", "Overlap", width), true, &OVERLAP, &mut r.overlap, seen);
    });
    row_of(ui, t, "Appearance", |ui, width| {
        list(ui, ("pagesetup.routing.appearance", "Appearance", width), true, &APPEARANCE, &mut r.appearance, seen);
    });
}

/// One size row of the Line jumps group: a slider and a box that hold one number from 0 to 10.
fn size_row(ui: &mut Ui, t: &Tokens, words: &str, id: &str, (text, last): (&mut String, &mut f64), seen: &mut Seen) {
    row_of(ui, t, words, |ui, width| {
        let gap = ui.spacing().item_spacing.x;
        let slider_w = (width - BOX_W - gap).max(48.0);
        ui.spacing_mut().slider_width = slider_w;
        let mut value = jump_size(text).unwrap_or(*last);
        let slider = ui.add_sized([slider_w, ROW_H], egui::Slider::new(&mut value, 0.0..=MOST_SIZE).show_value(false).trailing_fill(true));
        slider.widget_info(|| egui::WidgetInfo::slider(true, value, words.to_owned()));
        seen.push((format!("pagesetup.routing.{id}"), slider.rect, true));
        if slider.changed() {
            *text = number_text(value);
            *last = value;
        }
        let edit = ui.add_sized([BOX_W, ROW_H], egui::TextEdit::singleline(text).char_limit(12).font(theme::font(size::LABEL)));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{words}, 0 to {MOST_SIZE}")));
        seen.push((format!("pagesetup.routing.{id}.box"), edit.rect, true));
        if let Some(typed) = jump_size(text)
            && edit.changed()
        {
            *last = typed;
        }
    });
}

/// The Line jumps group: Add line jumps to, Line jump style, Vertical size and Horizontal size.
fn jumps_group(ui: &mut Ui, t: &Tokens, r: &mut Routing, seen: &mut Seen) {
    heading(ui, t, "Line jumps");
    row_of(ui, t, "Add line jumps to", |ui, width| {
        list(ui, ("pagesetup.routing.jump_to", "Add line jumps to", width), true, &JUMP_TO, &mut r.jump_to, seen);
    });
    row_of(ui, t, "Line jump style", |ui, width| {
        list(ui, ("pagesetup.routing.jump_style", "Line jump style", width), true, &JUMP_STYLES, &mut r.jump_style, seen);
    });
    size_row(ui, t, "Vertical size", "vertical_size", (&mut r.vertical, &mut r.sliders.0), seen);
    size_row(ui, t, "Horizontal size", "horizontal_size", (&mut r.horizontal, &mut r.sliders.1), seen);
}

/// The Other group: the two boxes, the line that says DiaCraft does not act on them, and Spacing....
fn other_group(ui: &mut Ui, t: &Tokens, r: &mut Routing, seen: &mut Seen) {
    heading(ui, t, "Other");
    for (id, words, tip, on) in
        [("plow", "Move other shapes away on drop", PLOW_TIP, &mut r.plow), ("split", "Enable connector splitting", SPLIT_TIP, &mut r.split)]
    {
        let boxed = ui.checkbox(on, RichText::new(words).font(theme::font(size::LABEL)).color(t.text)).on_hover_text(tip);
        seen.push((format!("pagesetup.routing.{id}"), boxed.rect, true));
    }
    ui.add(egui::Label::new(RichText::new(OTHER_NOTE).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
    ui.add_space(2.0);
    let spacing = ui.add(egui::Button::new(RichText::new("Spacing...").font(theme::font(size::LABEL))).min_size(vec2(96.0, 26.0)));
    seen.push(("pagesetup.routing.spacing".to_owned(), spacing.rect, true));
    if spacing.clicked() {
        r.dialog = Some(Spacing::open(r));
    }
}

/// Where the preview goes, `width` wide at most, under the Routing group or beside it.
fn preview_slot(ui: &mut Ui, width: f32) -> egui::Rect {
    let w = width.clamp(120.0, PREVIEW_MAX_W);
    let h = (w * 0.55).clamp(84.0, 150.0);
    let (rect, response) = ui.allocate_exact_size(vec2(w, h), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Preview of a connector routed round a shape"));
    rect
}

/// Draws the preview in its slot. **It is drawn after the groups**, whatever its place: the controls under it change the draft as they are drawn, and a preview that was drawn first would
/// show the frame before.
fn preview_paint(ui: &mut Ui, t: &Tokens, r: &mut Routing, rect: egui::Rect, seen: &mut Seen) {
    let style = r.route_style();
    let inputs = r.inputs();
    paint(&ui.painter_at(rect), rect, t, r.preview.scene(inputs), style);
    seen.push(("pagesetup.routing.preview".to_owned(), rect, true));
}

/// The tab: two columns when the dialog is wide enough (the Routing group over the Line jumps at the left, the preview over the Other group at the right), one column otherwise.
pub(super) fn tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, seen: &mut Seen) {
    let Some(r) = dialog.routing.as_mut() else {
        label(ui, t, "The routing settings of this page could not be read.");
        return;
    };
    let slot = if ui.available_width() >= WIDE_MIN {
        let beside = ui.available_width() - COLUMN_W - COLUMN_GAP;
        let slot = ui
            .horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(COLUMN_W);
                    routing_group(ui, t, r, seen);
                });
                ui.add_space(COLUMN_GAP);
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    preview_slot(ui, beside)
                })
                .inner
            })
            .inner;
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(COLUMN_W);
                jumps_group(ui, t, r, seen);
            });
            ui.add_space(COLUMN_GAP);
            ui.vertical(|ui| other_group(ui, t, r, seen));
        });
        slot
    } else {
        routing_group(ui, t, r, seen);
        ui.add_space(6.0);
        let width = ui.available_width();
        let slot = preview_slot(ui, width);
        ui.add_space(8.0);
        jumps_group(ui, t, r, seen);
        ui.add_space(8.0);
        other_group(ui, t, r, seen);
        slot
    };
    preview_paint(ui, t, r, slot, seen);
}

#[cfg(test)]
mod tests {
    use diacraft_engine::draw::Flow;

    use super::*;

    #[test]
    fn the_lists_have_the_words_the_commands_take_once_each() {
        for (name, rows) in [
            ("styles", &STYLES[..]),
            ("flows", &FLOWS[..]),
            ("simple", &SIMPLE_WAYS[..]),
            ("separate", &SEPARATE[..]),
            ("overlap", &OVERLAP[..]),
            ("appearance", &APPEARANCE[..]),
            ("jump to", &JUMP_TO[..]),
            ("jump styles", &JUMP_STYLES[..]),
        ] {
            let mut words: Vec<&str> = rows.iter().map(|r| r.word).collect();
            words.sort_unstable();
            words.dedup();
            assert_eq!(words.len(), rows.len(), "{name}: a word twice");
        }
        // Every style and direction is one the engine takes, and gives the style the words say.
        for style in STYLES {
            let ways = directions(style.word);
            let routes: Vec<RouteStyle> = if ways.is_empty() {
                vec![RouteStyle::choose(Some(style.word), None, RouteStyle::RightAngle).unwrap()]
            } else {
                ways.iter().map(|d| RouteStyle::choose(Some(style.word), Some(d.word), RouteStyle::RightAngle).unwrap()).collect()
            };
            assert_eq!(routes.len(), ways.len().max(1));
            assert!(routes.iter().all(|r| r.family() == style.word), "{}", style.word);
        }
        assert_eq!(FLOWS[0].word, Flow::TopToBottom.word());
    }

    #[test]
    fn a_jump_size_is_a_number_from_0_to_10() {
        for (text, want) in [
            ("0", Some(0.0)),
            ("10", Some(10.0)),
            (" 2.5 ", Some(2.5)),
            ("10.01", None),
            ("-0.1", None),
            ("abc", None),
            ("", None),
            ("1e400", None),
            ("nan", None),
        ] {
            assert_eq!(jump_size(text), want, "`{text}`");
        }
    }

    #[test]
    fn the_roots_values_are_the_same_lengths_in_both_units() {
        for group in 0..GROUPS {
            let (inches, millimetres) = (ROOT_INCHES[group], ROOT_MILLIMETRES[group]);
            // 3/8 inch and 7.5 mm are not equal lengths: the two roots are two sets of files, each in its own unit.
            assert!((inches * 25.4 - millimetres).abs() < 3.0, "group {group}: {inches} in against {millimetres} mm");
        }
        assert_eq!([0, 1, 2, 3].map(|g| Shown::Inches.root(g)), ["0.375", "0.25", "0.125", "0.125"]);
        assert_eq!([0, 1, 2, 3].map(|g| Shown::Millimetres.root(g)), ["7.5", "5", "2.5", "2.5"]);
        assert_eq!(Shown::Inches.root(9), "0", "a group that is not there has no root");
    }

    #[test]
    fn the_unit_shown_follows_the_unit_of_the_scale_on_paper() {
        for (unit, want) in [
            ("IN", Shown::Inches),
            ("", Shown::Inches),
            ("FT", Shown::Inches),
            ("junk", Shown::Inches),
            ("MM", Shown::Millimetres),
            ("cm", Shown::Millimetres),
            ("M", Shown::Millimetres),
        ] {
            assert_eq!(Shown::of(unit), want, "`{unit}`");
        }
    }

    /// What `page.routing` says of a new page at the root's values (a US drawing).
    fn root() -> Base {
        Base {
            style: "right_angle".to_owned(),
            direction: None,
            separate: "unrelated".to_owned(),
            overlap: "by_style".to_owned(),
            appearance: "straight".to_owned(),
            jump_code: 1,
            jump_style: 0,
            factor_x: 2.0 / 3.0,
            factor_y: 2.0 / 3.0,
            plow: false,
            split: true,
            spacing: [(0.375, 0.375), (0.25, 0.25), (0.125, 0.125), (0.125, 0.125)],
        }
    }

    fn draft() -> Routing {
        Routing::from_base(root(), Shown::Inches)
    }

    fn params(commands: &[(&'static str, Value)], id: &str) -> Value {
        commands.iter().find(|(c, _)| *c == id).map_or(Value::Null, |(_, p)| p.clone())
    }

    #[test]
    fn a_draft_nobody_touched_sends_nothing_and_the_texts_are_the_pages_values() {
        let r = draft();
        assert_eq!(r.commands(1).unwrap(), Vec::new());
        assert_eq!((r.vertical.as_str(), r.horizontal.as_str()), ("0.6667", "0.6667"));
        assert_eq!(r.spacing[0], ("0.375".to_owned(), "0.375".to_owned()));
        assert_eq!((r.style, r.direction, r.jump_to, r.jump_style), ("right_angle", "top_to_bottom", "1", "arc"));
    }

    #[test]
    fn a_page_set_routing_holds_only_the_settings_that_changed() {
        let mut r = draft();
        r.separate = "all";
        r.plow = true;
        r.spacing[3].0 = "0.5".to_owned();
        let commands = r.commands(7).unwrap();
        assert_eq!(commands.len(), 1, "no connector.line_jumps: no jump list or size was touched");
        assert_eq!(params(&commands, "page.set_routing"), json!({"separate": "all", "plow": true, "connector_to_shape": {"x": 0.5}, "page": 7}));
    }

    #[test]
    fn a_style_goes_with_its_direction_and_a_direction_alone_goes_with_its_style() {
        let mut r = draft();
        r.style = "flowchart";
        r.restyled();
        assert_eq!(params(&r.commands(1).unwrap(), "page.set_routing"), json!({"style": "flowchart", "direction": "top_to_bottom", "page": 1}));
        r.direction = "right_to_left";
        assert_eq!(params(&r.commands(1).unwrap(), "page.set_routing"), json!({"style": "flowchart", "direction": "right_to_left", "page": 1}));
        // A style that takes no direction sends none, whatever the box held before.
        r.style = "network";
        r.restyled();
        assert_eq!(params(&r.commands(1).unwrap(), "page.set_routing"), json!({"style": "network", "page": 1}));
        // The page already flows left to right as a tree: the same words again are no change, and another direction is.
        let mut tree = root();
        tree.style = "tree".to_owned();
        tree.direction = Some("left_to_right".to_owned());
        let mut r = Routing::from_base(tree, Shown::Inches);
        assert_eq!((r.style, r.direction), ("tree", "left_to_right"));
        assert_eq!(r.commands(1).unwrap(), Vec::new());
        r.direction = "bottom_to_top";
        assert_eq!(params(&r.commands(1).unwrap(), "page.set_routing"), json!({"style": "tree", "direction": "bottom_to_top", "page": 1}));
    }

    #[test]
    fn a_jump_list_or_size_that_changed_makes_a_connector_line_jumps_with_that_alone() {
        let mut r = draft();
        r.jump_to = "4";
        let commands = r.commands(2).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(params(&commands, "connector.line_jumps"), json!({"code": 4, "page": 2}));
        r.jump_style = "sides5";
        r.vertical = "3".to_owned();
        let commands = r.commands(2).unwrap();
        assert_eq!(params(&commands, "connector.line_jumps"), json!({"code": 4, "style": 7, "factor_y": 3.0, "page": 2}));
        // The code 1 of an arc the page holds as 1 is an arc as 0 is: choosing Arc is no change.
        let mut held = root();
        held.jump_style = 1;
        assert_eq!(Routing::from_base(held, Shown::Inches).commands(1).unwrap(), Vec::new());
    }

    #[test]
    fn a_size_that_is_no_number_from_0_to_10_is_refused_in_words_and_one_that_says_what_it_said_is_not_read() {
        let mut r = draft();
        r.horizontal = "11".to_owned();
        let words = r.commands(1).unwrap_err();
        assert!(words.contains("from 0 to 10"), "{words}");
        r.horizontal = "0.6667".to_owned();
        assert_eq!(r.commands(1).unwrap(), Vec::new(), "the same text is no change, though it is not 2/3 to the last place");
        r.horizontal = "0.66667".to_owned();
        assert_eq!(params(&r.commands(1).unwrap(), "connector.line_jumps"), json!({"factor_x": 0.66667, "page": 1}));
    }

    #[test]
    fn a_spacing_box_is_a_length_in_the_unit_shown_and_a_changed_one_that_is_none_is_refused() {
        let mut r = draft();
        r.spacing[2].1 = "1 cm".to_owned();
        let commands = r.commands(1).unwrap();
        let sent = params(&commands, "page.set_routing")["connector_to_connector"]["y"].as_f64().unwrap();
        assert!((sent - 10.0 / 25.4).abs() < 1.0e-9, "1 cm is {sent} inch");
        for bad in ["0", "-1", "abc", "", "1e400"] {
            r.spacing[2].1 = bad.to_owned();
            let words = r.commands(1).unwrap_err();
            assert!(words.contains("Connector to connector") && words.contains("Vertical") && words.contains("more than 0"), "`{bad}`: {words}");
        }
        // The same page in millimetres: a plain number is millimetres, and a box that says 0 and was 0 at the start is left alone.
        let mut metric = Routing::from_base(root(), Shown::Millimetres);
        metric.spacing[3].0 = "2.5".to_owned();
        let sent = params(&metric.commands(1).unwrap(), "page.set_routing")["connector_to_shape"]["x"].as_f64().unwrap();
        assert!((sent - 2.5 / 25.4).abs() < 1.0e-9);
        let mut zero = root();
        zero.spacing[3] = (0.0, 0.125);
        assert_eq!(Routing::from_base(zero, Shown::Inches).commands(1).unwrap(), Vec::new(), "a gap of 0 that a file holds is not looked at again");
    }

    #[test]
    fn what_the_engine_says_that_the_lists_do_not_have_is_shown_as_the_first_row() {
        let mut odd = root();
        odd.style = "hexagonal".to_owned();
        odd.separate = "sideways".to_owned();
        odd.jump_code = 9;
        odd.jump_style = 77;
        let r = Routing::from_base(odd, Shown::Inches);
        assert_eq!((r.style, r.separate, r.jump_to, r.jump_style), ("right_angle", "unrelated", "0", "arc"));
    }
}
