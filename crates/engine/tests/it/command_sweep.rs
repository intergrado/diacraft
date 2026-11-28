//! The every-command sweeps. Each registered command is run in each state of a session with empty,
//! missing and hostile parameters: it must never panic, must leave the session valid, and an error
//! must leave it exactly as it was.
//!
//! The sweeps classify the registry in tables below (what `{}` does, what runs without a document,
//! what undo takes back). A command in none of a table fails its sweep, so a new command has to be
//! looked at, not skipped.

use std::collections::HashSet;

use diacraft_engine::{EngineError, Session, command_specs, find_command, param_keys};
use diacraft_testkit::strategies::junk_values;
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, session_with_history, snapshot};

/// What `{}` does to a command in a session that has a document, both undo and redo steps and
/// nothing else special.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WithEmptyParams {
    Succeeds,
    NeedsParams,
    /// Refused as a failure, not as bad parameters: the command works on a file that this session's drawing does not have yet.
    NeedsAFile,
    /// Not enabled: it has nothing to work on yet (a paste with nothing on the clipboard).
    NothingYet,
}

const EMPTY_PARAMS: &[(&str, WithEmptyParams)] = &[
    ("app.version", WithEmptyParams::Succeeds),
    ("document.inspect", WithEmptyParams::Succeeds),
    ("file.new", WithEmptyParams::Succeeds),
    ("file.open", WithEmptyParams::NeedsParams),
    ("file.export", WithEmptyParams::NeedsParams),
    ("file.print", WithEmptyParams::NeedsParams),
    ("file.print_preview", WithEmptyParams::Succeeds),
    ("file.save", WithEmptyParams::NeedsAFile),
    ("file.save_as", WithEmptyParams::NeedsParams),
    // With no path it asks about the drawing's own file, as `file.save` does, which a new drawing does not have.
    ("file.check_compatibility", WithEmptyParams::NeedsAFile),
    ("file.status", WithEmptyParams::Succeeds),
    ("edit.undo", WithEmptyParams::Succeeds),
    ("edit.redo", WithEmptyParams::Succeeds),
    ("page.add", WithEmptyParams::Succeeds),
    // The first page that is not a background is copied.
    ("page.duplicate", WithEmptyParams::Succeeds),
    ("page.rename", WithEmptyParams::NeedsParams),
    ("page.presets", WithEmptyParams::Succeeds),
    ("page.setup", WithEmptyParams::Succeeds),
    ("page.size", WithEmptyParams::NeedsParams),
    ("page.orientation", WithEmptyParams::NeedsParams),
    ("page.scale", WithEmptyParams::NeedsParams),
    ("page.paper", WithEmptyParams::NeedsParams),
    ("page.auto_size", WithEmptyParams::NeedsParams),
    ("page.type", WithEmptyParams::NeedsParams),
    ("page.background", WithEmptyParams::NeedsParams),
    // Not enabled in a drawing of one page: there is nothing to delete or to put in another place.
    ("page.delete", WithEmptyParams::NothingYet),
    ("page.move", WithEmptyParams::NothingYet),
    ("command.batch", WithEmptyParams::NeedsParams),
    ("stencil.list", WithEmptyParams::Succeeds),
    ("stencil.open", WithEmptyParams::NeedsParams),
    ("stencil.close", WithEmptyParams::NeedsParams),
    ("stencil.search", WithEmptyParams::NeedsParams),
    ("stencil.quick", WithEmptyParams::Succeeds),
    ("stencil.add_quick", WithEmptyParams::NeedsParams),
    ("stencil.remove_quick", WithEmptyParams::NeedsParams),
    ("stencil.reset", WithEmptyParams::NeedsParams),
    ("stencil.icon", WithEmptyParams::NeedsParams),
    ("shape.drop", WithEmptyParams::NeedsParams),
    ("shape.move", WithEmptyParams::NeedsParams),
    ("shape.resize", WithEmptyParams::NeedsParams),
    ("shape.delete", WithEmptyParams::NeedsParams),
    ("shape.set_text", WithEmptyParams::NeedsParams),
    ("connector.add", WithEmptyParams::NeedsParams),
    ("connector.reconnect", WithEmptyParams::NeedsParams),
    ("connector.reverse_ends", WithEmptyParams::NeedsParams),
    ("connector.autoconnect", WithEmptyParams::NeedsParams),
    ("connector.neighbour", WithEmptyParams::NeedsParams),
    ("connector.style", WithEmptyParams::NeedsParams),
    ("connector.edit_path", WithEmptyParams::NeedsParams),
    ("connector.reroute", WithEmptyParams::NeedsParams),
    ("connector.reset", WithEmptyParams::NeedsParams),
    ("connector.line_jumps", WithEmptyParams::NeedsParams),
    // The connector is named: the empty call is refused in words.
    ("connector.path", WithEmptyParams::NeedsParams),
    ("shape.rotate", WithEmptyParams::NeedsParams),
    ("shape.flip", WithEmptyParams::NeedsParams),
    ("shape.order", WithEmptyParams::NeedsParams),
    ("shape.align", WithEmptyParams::NeedsParams),
    ("shape.format", WithEmptyParams::NeedsParams),
    ("text.format", WithEmptyParams::NeedsParams),
    ("shape.formatting", WithEmptyParams::NeedsParams),
    // The gradient, the shadow and the Quick Styles name the shapes they work on.
    ("shape.gradient", WithEmptyParams::NeedsParams),
    ("shape.shadow", WithEmptyParams::NeedsParams),
    ("shape.quick_style", WithEmptyParams::NeedsParams),
    ("shape.quick_styles", WithEmptyParams::NeedsParams),
    ("edit.copy", WithEmptyParams::NeedsParams),
    ("edit.find", WithEmptyParams::NeedsParams),
    ("edit.replace", WithEmptyParams::NeedsParams),
    ("edit.cut", WithEmptyParams::NeedsParams),
    ("edit.paste", WithEmptyParams::NothingYet),
    ("shape.duplicate", WithEmptyParams::NeedsParams),
    ("shape.group", WithEmptyParams::NeedsParams),
    ("shape.ungroup", WithEmptyParams::NeedsParams),
    ("shape.change", WithEmptyParams::NeedsParams),
    ("shape.union", WithEmptyParams::NeedsParams),
    ("shape.combine", WithEmptyParams::NeedsParams),
    ("shape.fragment", WithEmptyParams::NeedsParams),
    ("shape.intersect", WithEmptyParams::NeedsParams),
    ("shape.subtract", WithEmptyParams::NeedsParams),
    ("shape.join", WithEmptyParams::NeedsParams),
    ("shape.trim", WithEmptyParams::NeedsParams),
    ("shape.offset", WithEmptyParams::NeedsParams),
    ("shape.fit_curve", WithEmptyParams::NeedsParams),
    ("text.box", WithEmptyParams::NeedsParams),
    ("shape.draw", WithEmptyParams::NeedsParams),
    ("shape.close", WithEmptyParams::NeedsParams),
    ("select.by_type", WithEmptyParams::NeedsParams),
    ("select.by_region", WithEmptyParams::NeedsParams),
    ("select.choices", WithEmptyParams::Succeeds),
    ("text.block", WithEmptyParams::NeedsParams),
    ("text.block_move", WithEmptyParams::NeedsParams),
    ("text.block_resize", WithEmptyParams::NeedsParams),
    ("text.block_rotate", WithEmptyParams::NeedsParams),
    ("theme.list", WithEmptyParams::Succeeds),
    ("theme.apply", WithEmptyParams::NeedsParams),
    ("theme.variant", WithEmptyParams::NeedsParams),
    ("theme.colors", WithEmptyParams::Succeeds),
    ("theme.preview", WithEmptyParams::NeedsParams),
    ("shape.place", WithEmptyParams::NeedsParams),
    ("shape.pin", WithEmptyParams::NeedsParams),
    ("shape.size", WithEmptyParams::NeedsParams),
    ("shape.set_end", WithEmptyParams::NeedsParams),
    ("shape.set_length", WithEmptyParams::NeedsParams),
    ("shape.placement", WithEmptyParams::NeedsParams),
    // A query of the edges of the shapes named.
    ("shape.connection_edges", WithEmptyParams::NeedsParams),
    // Doubles or halves the connection points of the shapes named.
    ("shape.connection_points", WithEmptyParams::NeedsParams),
    // The Behavior dialog's Placement tab.
    ("shape.behavior", WithEmptyParams::NeedsParams),
    ("shape.set_behavior", WithEmptyParams::NeedsParams),
    ("template.list", WithEmptyParams::Succeeds),
    ("snap.settings", WithEmptyParams::Succeeds),
    // Naming no option changes nothing, which is no step and no error.
    ("snap.set", WithEmptyParams::Succeeds),
    ("snap.at", WithEmptyParams::NeedsParams),
    ("guide.add", WithEmptyParams::NeedsParams),
    ("guide.move", WithEmptyParams::NeedsParams),
    ("guide.delete", WithEmptyParams::NeedsParams),
    ("guide.list", WithEmptyParams::Succeeds),
    // A layer is named, a shape is named, a layer or the unreferenced ones are named: the empty call is refused in words.
    ("layer.list", WithEmptyParams::Succeeds),
    ("layer.add", WithEmptyParams::NeedsParams),
    ("layer.set", WithEmptyParams::NeedsParams),
    ("layer.assign", WithEmptyParams::NeedsParams),
    ("layer.delete", WithEmptyParams::NeedsParams),
    // A container: the gallery takes nothing and an empty container is dropped at the middle of the page; the others name the containers or shapes they work on.
    ("container.styles", WithEmptyParams::Succeeds),
    ("container.add", WithEmptyParams::Succeeds),
    ("container.info", WithEmptyParams::NeedsParams),
    ("container.style", WithEmptyParams::NeedsParams),
    ("container.heading", WithEmptyParams::NeedsParams),
    ("container.margin", WithEmptyParams::NeedsParams),
    ("container.fit", WithEmptyParams::NeedsParams),
    ("container.auto_resize", WithEmptyParams::NeedsParams),
    ("container.lock", WithEmptyParams::NeedsParams),
    ("container.disband", WithEmptyParams::NeedsParams),
    ("container.select_contents", WithEmptyParams::NeedsParams),
    ("container.add_member", WithEmptyParams::NeedsParams),
    ("container.attach_edge", WithEmptyParams::NeedsParams),
    // The swimlanes: a structure of three lanes is made from nothing and the page is read for its structures; the others name the lane, the phase or the orientation they work on.
    ("swimlane.info", WithEmptyParams::Succeeds),
    ("swimlane.create", WithEmptyParams::Succeeds),
    ("swimlane.insert", WithEmptyParams::NeedsParams),
    ("swimlane.insert_phase", WithEmptyParams::NeedsParams),
    ("swimlane.move", WithEmptyParams::NeedsParams),
    ("swimlane.resize", WithEmptyParams::NeedsParams),
    ("swimlane.orientation", WithEmptyParams::NeedsParams),
    ("swimlane.rotate_label", WithEmptyParams::NeedsParams),
    // A shape is named: the empty call is refused in words. The choices of the Field dialog and the update of every field take nothing.
    ("insert.picture", WithEmptyParams::NeedsParams),
    ("hyperlink.set", WithEmptyParams::NeedsParams),
    ("hyperlink.list", WithEmptyParams::NeedsParams),
    ("hyperlink.follow", WithEmptyParams::NeedsParams),
    // The comments: a comment needs its text and a reply, an edit and a delete name the comment; the list, the steps, the markers and the author take nothing.
    ("comment.add", WithEmptyParams::NeedsParams),
    ("comment.reply", WithEmptyParams::NeedsParams),
    ("comment.edit", WithEmptyParams::NeedsParams),
    ("comment.delete", WithEmptyParams::NeedsParams),
    ("comment.list", WithEmptyParams::Succeeds),
    ("comment.markers", WithEmptyParams::Succeeds),
    ("comment.next", WithEmptyParams::Succeeds),
    ("comment.previous", WithEmptyParams::Succeeds),
    ("comment.author", WithEmptyParams::Succeeds),
    // The Shape Data of a shape: a shape is named, and the empty call is refused in words; the types take nothing.
    ("shapedata.list", WithEmptyParams::NeedsParams),
    ("shapedata.set", WithEmptyParams::NeedsParams),
    ("shapedata.define", WithEmptyParams::NeedsParams),
    ("shapedata.types", WithEmptyParams::Succeeds),
    // Alt text: a shape and a master are named, and the empty call is refused in words; the page's reads the first page that is not a background; the check takes nothing.
    ("shape.alt_text", WithEmptyParams::NeedsParams),
    ("page.alt_text", WithEmptyParams::Succeeds),
    ("master.alt_text", WithEmptyParams::NeedsParams),
    ("review.check_accessibility", WithEmptyParams::Succeeds),
    // The callouts: the gallery takes nothing, and a callout with no shape and no place is one tied to nothing in the middle of the page; the others name a shape.
    ("callout.styles", WithEmptyParams::Succeeds),
    ("callout.insert", WithEmptyParams::Succeeds),
    ("callout.attach", WithEmptyParams::NeedsParams),
    ("callout.detach", WithEmptyParams::NeedsParams),
    ("callout.list", WithEmptyParams::NeedsParams),
    ("text.insert_field", WithEmptyParams::NeedsParams),
    ("text.field_choices", WithEmptyParams::Succeeds),
    ("text.field_sample", WithEmptyParams::NeedsParams),
    ("text.update_fields", WithEmptyParams::Succeeds),
    ("text.insert", WithEmptyParams::NeedsParams),
    ("window.settings", WithEmptyParams::Succeeds),
    // Naming no setting changes nothing, which is no step and no error.
    ("window.settings.set", WithEmptyParams::Succeeds),
    ("autoconnect.settings", WithEmptyParams::Succeeds),
    // The box must be named.
    ("autoconnect.set", WithEmptyParams::NeedsParams),
    // Re-Layout Page, Auto Space and the page breaks: the three that work on shapes are not enabled in a drawing with none, and the settings of a page are read with
    // `{}`; what a space or a box is to be must be named.
    ("page.relayout", WithEmptyParams::NothingYet),
    ("page.layout", WithEmptyParams::Succeeds),
    ("shape.autospace", WithEmptyParams::NothingYet),
    ("page.spacing", WithEmptyParams::NeedsParams),
    ("page.avoid_page_breaks", WithEmptyParams::NeedsParams),
    // The page's routing: the settings are read with `{}`, and what a call sets must be named: the empty call is refused in words.
    ("page.routing", WithEmptyParams::Succeeds),
    ("page.set_routing", WithEmptyParams::NeedsParams),
    ("shape.move_off_page_breaks", WithEmptyParams::NothingYet),
    ("page.breaks", WithEmptyParams::Succeeds),
    ("page_breaks.settings", WithEmptyParams::Succeeds),
    ("page_breaks.set", WithEmptyParams::NeedsParams),
    // The Organization Chart: the chart is read with `{}`; the layouts of a whole chart are not enabled in a drawing with no position; the others name a position.
    ("orgchart.info", WithEmptyParams::Succeeds),
    ("orgchart.layout", WithEmptyParams::NothingYet),
    ("orgchart.relayout", WithEmptyParams::NothingYet),
    ("orgchart.arrange", WithEmptyParams::NeedsParams),
    ("orgchart.link", WithEmptyParams::NeedsParams),
    ("orgchart.picture_set", WithEmptyParams::NeedsParams),
    ("orgchart.picture_delete", WithEmptyParams::NeedsParams),
    ("orgchart.picture_hide", WithEmptyParams::NeedsParams),
    // The ShapeSheet's queries: each names the sheet it reads, and the empty call is refused in words.
    ("sheet.read", WithEmptyParams::NeedsParams),
    ("sheet.cell", WithEmptyParams::NeedsParams),
    ("sheet.sections", WithEmptyParams::NeedsParams),
    ("sheet.evaluate", WithEmptyParams::NeedsParams),
    ("sheet.set_cell", WithEmptyParams::NeedsParams),
    ("sheet.set_cells", WithEmptyParams::NeedsParams),
    // The ShapeSheet's rows and sections: each names the sheet and the section, and the empty call is refused in words.
    ("sheet.add_row", WithEmptyParams::NeedsParams),
    ("sheet.delete_row", WithEmptyParams::NeedsParams),
    ("sheet.add_section", WithEmptyParams::NeedsParams),
    ("sheet.delete_section", WithEmptyParams::NeedsParams),
];

/// The commands that are enabled in a session with no document.
const RUNS_WITHOUT_A_DOCUMENT: &[&str] =
    &["app.version", "file.new", "stencil.list", "stencil.search", "stencil.quick", "page.presets", "theme.list", "template.list", "comment.author"];

/// The commands that are enabled in a session with no document and still need params, so `{}` is refused as bad params and not as disabled.
const NEEDS_PARAMS_WITHOUT_A_DOCUMENT: &[&str] = &["file.open", "stencil.open", "stencil.close", "stencil.search", "stencil.icon"];

/// A command that only reads: it changes nothing and is not journaled.
const QUERIES: &[&str] = &[
    "app.version",
    "document.inspect",
    "file.print_preview",
    "file.status",
    "file.check_compatibility",
    "shape.formatting",
    "shape.quick_styles",
    "stencil.list",
    "stencil.search",
    "stencil.quick",
    "stencil.icon",
    "page.presets",
    "page.setup",
    "text.block",
    "theme.list",
    "theme.colors",
    "theme.preview",
    "shape.placement",
    "shape.behavior",
    "shape.connection_edges",
    "template.list",
    "snap.settings",
    "snap.at",
    "guide.list",
    "layer.list",
    "container.styles",
    "container.info",
    "container.select_contents",
    "swimlane.info",
    "hyperlink.list",
    "hyperlink.follow",
    "comment.list",
    "comment.next",
    "comment.previous",
    "comment.author",
    "shapedata.list",
    "shapedata.types",
    "review.check_accessibility",
    "callout.styles",
    "callout.list",
    "text.field_choices",
    "text.field_sample",
    "window.settings",
    "autoconnect.settings",
    "page.layout",
    "page.routing",
    "page.breaks",
    "page_breaks.settings",
    "orgchart.info",
    "sheet.read",
    "sheet.cell",
    "sheet.sections",
    "sheet.evaluate",
    "connector.neighbour",
    "connector.path",
    "select.by_type",
    "select.by_region",
    "select.choices",
    "edit.find",
];

/// Commands that change the document without being an undo step, with the reason. A command that is
/// neither here, in `QUERIES` nor in `undoable_samples` fails `every_command_undoes_exactly`.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "file.new",
        "replaces the session's document and starts a fresh history; M0 has no save, so there is no saved state for undo to protect (revisit with file save)",
    ),
    (
        "file.open",
        "replaces the session's document and starts a fresh history, as file.new does; M2 has no save, so there is no saved state for undo to protect, and a file that cannot be opened leaves the open document as it is (revisit with file save)",
    ),
    (
        "file.save",
        "writes the file and marks the drawing saved; the document is not changed, so there is nothing to take back (undo after a save makes the drawing unsaved again)",
    ),
    ("file.save_as", "as file.save, to a path the caller gives: the document is not changed"),
    ("file.export", "writes a picture of one page to a path the caller gives; the document is not changed and no file of the drawing is written"),
    (
        "file.print",
        "writes the sheets a print would take to a PDF at a path the caller gives; the document is not changed and no file of the drawing is written",
    ),
    (
        "stencil.open",
        "adds a stencil file to the session's open stencils, which are not part of the document or its history; a failed batch or command puts them back",
    ),
    (
        "stencil.close",
        "takes a stencil out of the session's open stencils: not part of the document or its history, and a drawing that took masters from it keeps them",
    ),
    (
        "text.update_fields",
        "works the text of the fields out again, and a call that finds every field showing what its row gives (the sweep's drawing has none) changes nothing and is no step; a call that changes a text is one undo step (tests/it/fields.rs)",
    ),
    ("edit.undo", "moves back along the history; the step it takes back is put again by edit.redo"),
    ("edit.redo", "moves forward along the history; the step it puts again is taken back by edit.undo"),
    ("edit.copy", "puts copies of shapes aside on the session's clipboard; the document is not changed, so there is nothing to take back"),
    (
        "window.settings.set",
        "changes the drawing's window settings (grid, rulers, guides, connection points) and marks it unsaved, but is no undo step by the owner's ruling: undo and redo leave the settings alone",
    ),
    (
        "comment.markers",
        "turns the markers of the comments on or off (Show Markup) and marks the drawing unsaved, but is no undo step by the owner's ruling for the View settings: undo and redo leave the setting alone",
    ),
    (
        "autoconnect.set",
        "changes the drawing's AutoConnect box and marks it unsaved, but is no undo step, which extends the owner's ruling for the View settings: undo and redo leave the box alone",
    ),
    (
        "page_breaks.set",
        "changes the drawing's Page Breaks box (View, Show) and marks it unsaved, but is no undo step, which extends the owner's ruling for the View settings: undo and redo leave the box alone",
    ),
    (
        "stencil.add_quick",
        "changes the Quick Shapes of a stencil in the drawing's own data and marks it unsaved, but is no undo step (a setting, as the View boxes are): undo and redo leave the choice alone",
    ),
    (
        "stencil.remove_quick",
        "changes the Quick Shapes of a stencil in the drawing's own data and marks it unsaved, but is no undo step (a setting, as the View boxes are): undo and redo leave the choice alone",
    ),
    (
        "stencil.reset",
        "gives a stencil its own Quick Shapes back in the drawing's own data and marks it unsaved, but is no undo step (a setting, as the View boxes are): undo and redo leave the choice alone",
    ),
    (
        "snap.set",
        "changes the drawing's Snap and Glue options, Dynamic Grid and snap strengths and marks it unsaved, but is no undo step, which extends the owner's ruling for the View settings: undo and redo leave them alone",
    ),
];

/// A picture file for the sweep of `insert.picture`: a PNG of 8 by 8 pixels, written once to a name of this process's own.
fn sweep_picture() -> String {
    let path = std::env::temp_dir().join(format!("diacraft-engine-sweep-{}.png", std::process::id()));
    std::fs::write(&path, crate::pictures::real_png(8, 8)).unwrap();
    path.to_str().unwrap().to_owned()
}

/// Commands that change the document through history, each with params that must succeed and change
/// something.
pub(crate) fn undoable_samples() -> Vec<(&'static str, Value)> {
    vec![
        ("page.add", json!({})),
        ("page.add", json!({"name": "Extra"})),
        ("page.rename", json!({"id": 1, "name": "Renamed"})),
        ("page.add", json!({"name": "Copy", "like": 1})),
        ("page.add", json!({"background": true})),
        ("page.duplicate", json!({"id": 2})),
        ("page.type", json!({"type": "background", "page": 2})),
        ("page.background", json!({"background": 3, "page": 1})),
        ("page.size", json!({"preset": "a4"})),
        ("page.orientation", json!({"orientation": "portrait"})),
        ("page.scale", json!({"preset": "arch_1_4"})),
        ("page.paper", json!({"kind": 5})),
        ("page.auto_size", json!({"on": true})),
        (
            "page.set_routing",
            json!({"style": "flowchart", "direction": "left_to_right", "separate": "all", "connector_to_shape": {"x": 0.3, "y": 0.35}}),
        ),
        ("page.delete", json!({"id": 2})),
        ("page.move", json!({"id": 2, "to": 0})),
        (
            "command.batch",
            json!({"commands": [
                {"command": "page.add", "params": {"name": "Third"}},
                {"command": "page.add"},
                {"command": "page.rename", "params": {"id": 1, "name": "First"}},
                {"command": "document.inspect"},
            ], "label": "Three edits"}),
        ),
        ("shape.drop", json!({"master": "hexagon", "page": 1, "x": 4, "y": 4})),
        ("shape.move", json!({"ids": [1], "dx": 1, "dy": 1})),
        ("shape.resize", json!({"id": 1, "x": 1, "y": 1, "width": 3, "height": 2})),
        ("shape.delete", json!({"ids": [2]})),
        ("shape.set_text", json!({"id": 1, "text": "Hello"})),
        ("edit.replace", json!({"what": "Hello", "with": "Jello", "scope": "all"})),
        ("connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})),
        ("connector.reconnect", json!({"id": 3, "end": "end", "target": {"x": 9, "y": 1}})),
        ("connector.reverse_ends", json!({"ids": [3]})),
        ("connector.autoconnect", json!({"from": 1, "direction": "right"})),
        ("connector.autoconnect", json!({"from": 2, "direction": "down", "master": "hexagon"})),
        ("connector.style", json!({"style": "curved", "ids": [3]})),
        ("connector.edit_path", json!({"id": 3, "leg": 1, "to": 3.5})),
        ("connector.reroute", json!({"ids": [3], "mode": "never"})),
        ("connector.reset", json!({"ids": [3]})),
        ("connector.line_jumps", json!({"code": 2})),
        ("layer.add", json!({"name": "Sweep"})),
        ("layer.set", json!({"layer": 0, "visible": false})),
        ("layer.assign", json!({"ids": [1], "layers": [0]})),
        ("layer.delete", json!({"layer": 0})),
        ("container.add", json!({"x": 4, "y": 4})),
        ("container.style", json!({"ids": [1], "style": "tint"})),
        ("container.heading", json!({"ids": [1], "rotate": true})),
        ("container.margin", json!({"ids": [1], "margin": "wide"})),
        ("container.fit", json!({"ids": [1]})),
        ("container.auto_resize", json!({"ids": [1], "mode": "fit"})),
        ("container.lock", json!({"ids": [1]})),
        ("container.disband", json!({"ids": [1]})),
        ("container.add_member", json!({"ids": [1]})),
        ("container.attach_edge", json!({"id": 2, "container": 1, "edge": "left"})),
        ("swimlane.create", json!({"x": 1, "y": 8, "lanes": 2, "phases": 2})),
        ("swimlane.insert", json!({"lane": 3})),
        ("swimlane.insert_phase", json!({"phase": 6})),
        ("swimlane.move", json!({"lane": 4, "to": 0})),
        ("swimlane.resize", json!({"id": 3, "size": 2.0})),
        ("swimlane.orientation", json!({"orientation": "vertical"})),
        ("swimlane.rotate_label", json!({"ids": [3]})),
        ("insert.picture", json!({"path": sweep_picture()})),
        ("hyperlink.set", json!({"id": 1, "links": [{"address": "https://sweep.example/", "description": "Sweep"}]})),
        ("comment.add", json!({"text": "Sweep"})),
        ("comment.reply", json!({"comment": 1, "text": "Sweep reply"})),
        ("comment.edit", json!({"comment": 1, "text": "Sweep, edited"})),
        ("comment.delete", json!({"comment": 1})),
        ("shapedata.define", json!({"id": 1, "fields": [{"label": "Sweep", "type": "number", "value": "3"}]})),
        ("shapedata.set", json!({"id": 1, "row": "Setup", "value": "4"})),
        ("shape.alt_text", json!({"id": 1, "title": "Sweep", "description": "A sweep shape"})),
        ("page.alt_text", json!({"title": "Sweep"})),
        ("master.alt_text", json!({"id": 1, "title": "Sweep"})),
        ("sheet.set_cell", json!({"id": 1, "cell": "Width", "formula": "3"})),
        ("sheet.set_cells", json!({"writes": [{"id": 1, "cell": "Width", "formula": "2.5"}, {"page": 1, "cell": "PageHeight", "formula": "9"}]})),
        ("sheet.add_row", json!({"id": 1, "section": "Scratch"})),
        ("sheet.delete_row", json!({"id": 1, "section": "Scratch", "row": 1})),
        ("sheet.add_section", json!({"id": 1, "section": "Prop"})),
        ("sheet.delete_section", json!({"id": 1, "section": "Scratch"})),
        ("callout.insert", json!({"ids": [1], "style": "speech_box"})),
        ("callout.attach", json!({"id": 3, "target": 1})),
        ("callout.detach", json!({"id": 3, "x": 7, "y": 7})),
        ("text.insert_field", json!({"id": 1, "category": "page", "field": "page_number"})),
        ("text.insert", json!({"id": 1, "text": "x"})),
        ("shape.rotate", json!({"ids": [1], "by": 30})),
        ("shape.flip", json!({"ids": [1, 2], "axis": "horizontal"})),
        ("shape.order", json!({"ids": [1], "to": "front"})),
        ("shape.align", json!({"ids": [1, 2], "how": "left"})),
        ("shape.format", json!({"ids": [1, 3], "fill": "#336699", "line": "#CC0000", "weight": 1.5, "end_arrow": 13})),
        (
            "shape.gradient",
            json!({"ids": [1, 3], "angle": 45, "stops": [{"position": 0, "color": "#336699"}, {"position": 1, "color": "#CC0000", "transparency": 20}]}),
        ),
        ("shape.shadow", json!({"ids": [1, 3], "preset": "bottom_right"})),
        ("shape.quick_style", json!({"ids": [1, 2], "row": 2, "column": 3})),
        ("text.format", json!({"ids": [1], "bold": true, "size": 11, "color": "#202020", "align": "left"})),
        ("edit.cut", json!({"ids": [2]})),
        ("edit.paste", json!({})),
        ("shape.duplicate", json!({"ids": [1, 2, 3]})),
        ("shape.group", json!({"ids": [1, 2]})),
        ("shape.ungroup", json!({"ids": [4]})),
        ("shape.change", json!({"ids": [1], "master": "ellipse"})),
        ("shape.union", json!({"ids": [1, 2]})),
        ("shape.combine", json!({"ids": [1, 2]})),
        ("shape.fragment", json!({"ids": [1, 2]})),
        ("shape.intersect", json!({"ids": [1, 2]})),
        ("shape.subtract", json!({"ids": [1, 2]})),
        ("shape.join", json!({"ids": [1, 2]})),
        ("shape.trim", json!({"ids": [1, 2]})),
        ("shape.offset", json!({"ids": [1, 2], "distance": 0.25})),
        ("shape.fit_curve", json!({"ids": [1, 2], "tolerance": 0.5})),
        ("text.box", json!({"page": 1, "x": 4, "y": 4, "text": "Hi"})),
        ("shape.draw", json!({"page": 1, "tool": "pencil", "points": [[8, 1], [9, 1.5], [9.5, 3]]})),
        ("shape.draw", json!({"page": 1, "tool": "rectangle", "points": [[8, 5], [10, 6]], "shift": true})),
        ("shape.draw", json!({"page": 1, "tool": "line", "points": [[6, 9], [8, 9]], "continue": 4})),
        ("shape.close", json!({"id": 4})),
        ("text.block_move", json!({"id": 1, "dx": 0.5, "dy": 0.25})),
        ("text.block_resize", json!({"id": 1, "x": 1, "y": 1, "width": 2, "height": 1})),
        ("text.block_rotate", json!({"id": 1, "by": 30})),
        ("theme.apply", json!({"theme": "tidepool", "variant": 1, "scope": "all"})),
        ("theme.variant", json!({"colors": 2})),
        ("shape.place", json!({"ids": [1, 2], "x": 6.5})),
        ("shape.pin", json!({"ids": [1, 2], "at": "top-left"})),
        ("shape.size", json!({"ids": [1, 2], "width": 3, "height": 2})),
        ("shape.set_end", json!({"id": 3, "end": "end", "x": 9, "y": 3})),
        ("shape.set_length", json!({"id": 3, "length": 2.5})),
        ("shape.set_behavior", json!({"ids": [1], "placement": "none", "route_through_x": true})),
        ("page.relayout", json!({"style": "flowchart", "direction": "top_to_bottom", "spacing": 0.9})),
        ("orgchart.layout", json!({"layout": "left_to_right"})),
        ("orgchart.relayout", json!({})),
        ("orgchart.arrange", json!({"id": 1, "arrangement": "vertical_right"})),
        ("orgchart.link", json!({"id": 4, "manager": 2})),
        ("orgchart.picture_set", json!({"id": 2, "path": sweep_picture()})),
        ("orgchart.picture_delete", json!({"id": 2})),
        ("orgchart.picture_hide", json!({"id": 2})),
        ("shape.autospace", json!({"horizontal": 7.5, "vertical": 7.5})),
        ("page.spacing", json!({"horizontal": 0.8, "vertical": 0.6})),
        ("page.avoid_page_breaks", json!({"on": true})),
        ("shape.move_off_page_breaks", json!({})),
        ("guide.add", json!({"page": 1, "orientation": "vertical", "at": 3})),
        ("guide.move", json!({"id": 1, "at": 4})),
        ("guide.delete", json!({"id": 1})),
        ("connector.autoconnect", json!({"from": 1, "direction": "up", "existing": true})),
        ("connector.autoconnect", json!({"from": 1, "direction": "up", "to": 2, "to_point": 1})),
        // Both shapes get points in the middle of their portions, and the connector glued to them stays glued.
        ("shape.connection_points", json!({"ids": [1, 2], "action": "double"})),
    ]
}

/// An org chart: an Executive (shape 1), a Manager dropped on it and so linked (shape 2, connector 3), another Manager apart (shape 4), and the first pulled out of line so that a layout has
/// something to move; the picture commands that take a picture away or hide it start from one on shape 2.
fn org_chart_setup(id: &str) -> Vec<(&'static str, Value)> {
    let mut setup = vec![
        ("shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 4, "y": 8})),
        ("shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": 4, "y": 8})),
        ("shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": 9, "y": 3})),
        ("shape.move", json!({"ids": [2], "dx": 3.0, "dy": -2.0})),
    ];
    if matches!(id, "orgchart.picture_delete" | "orgchart.picture_hide") {
        setup.push(("orgchart.picture_set", json!({"id": 2, "path": sweep_picture()})));
    }
    setup
}

/// The commands that must have run before a sample: the editing commands need shapes. Two shapes (1 and 2) and a connector between them (3).
/// What the ShapeSheet's commands start from (and 5): a cell to write, a section to add: the shape alone (shape 1); a row to add or delete, a section to delete: the
/// shape with a Scratch section of one row. `None` for any other command.
fn sheet_setup(id: &str) -> Option<Vec<(&'static str, Value)>> {
    let shape = ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    match id {
        // Alt text to set: a shape (shape 1), and with it the master it is dropped from (master 1).
        "sheet.set_cell" | "sheet.set_cells" | "sheet.add_section" | "shape.alt_text" | "master.alt_text" => Some(vec![shape]),
        "sheet.add_row" | "sheet.delete_row" | "sheet.delete_section" => {
            Some(vec![shape, ("sheet.add_section", json!({"id": 1, "section": "Scratch"}))])
        }
        _ => None,
    }
}

pub(crate) fn setup_for(id: &str) -> Vec<(&'static str, Value)> {
    // A variant is a variant of a theme the page has.
    if id == "theme.variant" {
        return vec![("theme.apply", json!({"theme": "tidepool"}))];
    }
    // A page is given a background page: the drawing has one (the third page).
    if id == "page.background" {
        return vec![("page.add", json!({"background": true}))];
    }
    // A guide to move or delete: the first shape of the page.
    if matches!(id, "guide.move" | "guide.delete") {
        return vec![("guide.add", json!({"page": 1, "orientation": "horizontal", "at": 2}))];
    }
    // A comment to answer, to edit or to take away: one on the page (comment 1).
    if matches!(id, "comment.reply" | "comment.edit" | "comment.delete") {
        return vec![("comment.add", json!({"text": "Setup"}))];
    }
    // A layer to change, to put a shape on or to remove, and for the assignment a shape (shape 1).
    if matches!(id, "layer.set" | "layer.assign" | "layer.delete") {
        let mut setup = vec![("layer.add", json!({"name": "Setup"}))];
        if id == "layer.assign" {
            setup.push(("shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})));
        }
        return setup;
    }
    if let Some(setup) = sheet_setup(id) {
        return setup;
    }
    // A value to enter: a shape with a field (shape 1).
    if id == "shapedata.set" {
        return vec![
            ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})),
            ("shapedata.define", json!({"id": 1, "fields": [{"label": "Setup", "name": "Setup", "type": "number"}]})),
        ];
    }
    // A container to work on (the shape 1, empty, in the middle of the page); `container.fit` and `container.attach_edge` need a member of it (the shape 2); `container.add_member`
    // needs a shape lying over a container that holds nothing (the shapes 1 and 2).
    if id == "container.add_member" {
        return vec![("shape.drop", json!({"master": "rectangle", "page": 1, "x": 4, "y": 4})), ("container.add", json!({"x": 4, "y": 4}))];
    }
    if matches!(id, "container.fit" | "container.attach_edge") {
        let mut setup = vec![("container.add", json!({"x": 4, "y": 4})), ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 4, "y": 4}))];
        if id == "container.fit" {
            setup.push(("container.auto_resize", json!({"ids": [1], "mode": "none"})));
            setup.push(("shape.move", json!({"ids": [2], "dx": 0.6, "dy": 0.0})));
        }
        return setup;
    }
    // A structure of two lanes and two phases to work on: the frame is the shape 1, its list of lanes 2, the lanes 3 and 4, the list of phases 5 and the phases 6 and 7.
    if id.starts_with("swimlane.") && id != "swimlane.create" && id != "swimlane.info" {
        return vec![("swimlane.create", json!({"x": 1, "y": 8, "lanes": 2, "phases": 2}))];
    }
    if matches!(id, "container.style" | "container.heading" | "container.margin" | "container.auto_resize" | "container.lock" | "container.disband") {
        return vec![("container.add", json!({"x": 4, "y": 4}))];
    }
    // A callout to tie and untie: the two shapes and a callout tied to nothing (shape 3).
    if matches!(id, "callout.attach" | "callout.detach") {
        return vec![
            ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})),
            ("shape.drop", json!({"master": "ellipse", "page": 1, "x": 6, "y": 4})),
            ("callout.insert", json!({"x": 5, "y": 7})),
        ];
    }
    // A page two sheets across with a shape that lies across the break between them (a sheet is 10.5 to 11 inches wide, so 10 to 11.5 is across either): the shape 1.
    if id == "shape.move_off_page_breaks" {
        return vec![
            ("page.size", json!({"width": 20, "height": 7.5})),
            ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 10.75, "y": 4})),
        ];
    }
    if id.starts_with("orgchart.") {
        return org_chart_setup(id);
    }
    if !(id.starts_with("shape.")
        || id == "page.relayout"
        || id.starts_with("callout.")
        || id.starts_with("connector.")
        || id.starts_with("hyperlink.")
        || id.starts_with("shapedata.")
        || id.starts_with("text.")
        || matches!(id, "edit.cut" | "edit.paste" | "edit.replace"))
    {
        return Vec::new();
    }
    // The shape operations need shapes that overlap: a second rectangle across the first (the others have an ellipse far from it).
    let overlapping = matches!(
        id,
        "shape.union"
            | "shape.combine"
            | "shape.fragment"
            | "shape.intersect"
            | "shape.subtract"
            | "shape.join"
            | "shape.trim"
            | "shape.offset"
            | "shape.fit_curve"
    );
    let second = if overlapping {
        json!({"master": "rectangle", "page": 1, "x": 2.5, "y": 2.3})
    } else {
        json!({"master": "ellipse", "page": 1, "x": 6, "y": 4})
    };
    let mut setup = vec![("shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})), ("shape.drop", second)];
    if id != "connector.add" {
        setup.push(("connector.add", json!({"from": {"id": 1}, "to": {"id": 2}})));
    }
    // Something to paste: shapes 1 and 2 and the connector between them, on the clipboard. Something to ungroup: group 4 of shapes 1 and 2.
    if id == "edit.paste" {
        setup.push(("edit.copy", json!({"ids": [1, 2, 3]})));
    }
    if id == "shape.ungroup" {
        setup.push(("shape.group", json!({"ids": [1, 2]})));
    }
    // A path to go on from and to close: a line (shape 4, after the two shapes and the connector).
    if matches!(id, "shape.close" | "shape.draw") {
        setup.push(("shape.draw", json!({"page": 1, "tool": "line", "points": [[1, 8], [5, 7]]})));
        setup.push(("shape.draw", json!({"page": 1, "tool": "line", "points": [[5, 7], [6, 9]], "continue": 4})));
    }
    // Something to type into: a connector with its end free (the one between them has both ends glued).
    if matches!(id, "shape.set_end" | "shape.set_length") {
        setup.push(("connector.reconnect", json!({"id": 3, "end": "end", "target": {"x": 9, "y": 1}})));
    }
    // A Quick Style is taken from the page's theme: the page has one.
    if id == "shape.quick_style" {
        setup.insert(0, ("theme.apply", json!({"theme": "tidepool"})));
    }
    // Something to replace: a word in the text of shape 1.
    if id == "edit.replace" {
        setup.push(("shape.set_text", json!({"id": 1, "text": "Hello"})));
    }
    // Something to reset: a path moved by hand.
    if id == "connector.reset" {
        setup.push(("connector.edit_path", json!({"id": 3, "leg": 1, "to": 3.5})));
    }
    setup
}

fn rich_session() -> Session {
    let mut s = session_with_history();
    run(&mut s, "edit.undo", json!({}));
    s
}

fn is_internal(e: &EngineError) -> bool {
    matches!(e, EngineError::Internal { .. })
}

#[test]
fn every_command_has_unique_id_and_doc() {
    let specs = command_specs();
    let mut seen = HashSet::new();
    for c in specs {
        assert!(seen.insert(c.id), "duplicate command id `{}`", c.id);
        assert!(!c.label.trim().is_empty(), "`{}` has no label", c.id);
        let segments: Vec<&str> = c.id.split('.').collect();
        assert!(
            // A word may hold `_` after its first letter: M3's plan names `shape.set_text`.
            segments.len() >= 2
                && segments
                    .iter()
                    .all(|s| s.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')),
            "`{}`: ids are dotted, namespaced words",
            c.id
        );
        assert!(c.menu.iter().all(|m| !m.trim().is_empty()), "`{}` has an empty menu name", c.id);
        assert!(c.shortcut.is_none_or(|s| !s.trim().is_empty()), "`{}` has an empty shortcut", c.id);
        if let Err(e) = param_keys(c.params) {
            panic!("`{}`: its params doc `{}` is not a list of `name: description` entries: {e}", c.id, c.params);
        }
        assert_eq!(find_command(c.id).map(|f| f.id), Some(c.id));
    }
    let mut ids: Vec<&str> = specs.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "app.version",
            "autoconnect.set",
            "autoconnect.settings",
            "callout.attach",
            "callout.detach",
            "callout.insert",
            "callout.list",
            "callout.styles",
            "command.batch",
            "comment.add",
            "comment.author",
            "comment.delete",
            "comment.edit",
            "comment.list",
            "comment.markers",
            "comment.next",
            "comment.previous",
            "comment.reply",
            "connector.add",
            "connector.autoconnect",
            "connector.edit_path",
            "connector.line_jumps",
            "connector.neighbour",
            "connector.path",
            "connector.reconnect",
            "connector.reroute",
            "connector.reset",
            "connector.reverse_ends",
            "connector.style",
            "container.add",
            "container.add_member",
            "container.attach_edge",
            "container.auto_resize",
            "container.disband",
            "container.fit",
            "container.heading",
            "container.info",
            "container.lock",
            "container.margin",
            "container.select_contents",
            "container.style",
            "container.styles",
            "document.inspect",
            "edit.copy",
            "edit.cut",
            "edit.find",
            "edit.paste",
            "edit.redo",
            "edit.replace",
            "edit.undo",
            "file.check_compatibility",
            "file.export",
            "file.new",
            "file.open",
            "file.print",
            "file.print_preview",
            "file.save",
            "file.save_as",
            "file.status",
            "guide.add",
            "guide.delete",
            "guide.list",
            "guide.move",
            "hyperlink.follow",
            "hyperlink.list",
            "hyperlink.set",
            "insert.picture",
            "layer.add",
            "layer.assign",
            "layer.delete",
            "layer.list",
            "layer.set",
            "master.alt_text",
            "orgchart.arrange",
            "orgchart.info",
            "orgchart.layout",
            "orgchart.link",
            "orgchart.picture_delete",
            "orgchart.picture_hide",
            "orgchart.picture_set",
            "orgchart.relayout",
            "page.add",
            "page.alt_text",
            "page.auto_size",
            "page.avoid_page_breaks",
            "page.background",
            "page.breaks",
            "page.delete",
            "page.duplicate",
            "page.layout",
            "page.move",
            "page.orientation",
            "page.paper",
            "page.presets",
            "page.relayout",
            "page.rename",
            "page.routing",
            "page.scale",
            "page.set_routing",
            "page.setup",
            "page.size",
            "page.spacing",
            "page.type",
            "page_breaks.set",
            "page_breaks.settings",
            "review.check_accessibility",
            "select.by_region",
            "select.by_type",
            "select.choices",
            "shape.align",
            "shape.alt_text",
            "shape.autospace",
            "shape.behavior",
            "shape.change",
            "shape.close",
            "shape.combine",
            "shape.connection_edges",
            "shape.connection_points",
            "shape.delete",
            "shape.draw",
            "shape.drop",
            "shape.duplicate",
            "shape.fit_curve",
            "shape.flip",
            "shape.format",
            "shape.formatting",
            "shape.fragment",
            "shape.gradient",
            "shape.group",
            "shape.intersect",
            "shape.join",
            "shape.move",
            "shape.move_off_page_breaks",
            "shape.offset",
            "shape.order",
            "shape.pin",
            "shape.place",
            "shape.placement",
            "shape.quick_style",
            "shape.quick_styles",
            "shape.resize",
            "shape.rotate",
            "shape.set_behavior",
            "shape.set_end",
            "shape.set_length",
            "shape.set_text",
            "shape.shadow",
            "shape.size",
            "shape.subtract",
            "shape.trim",
            "shape.ungroup",
            "shape.union",
            "shapedata.define",
            "shapedata.list",
            "shapedata.set",
            "shapedata.types",
            "sheet.add_row",
            "sheet.add_section",
            "sheet.cell",
            "sheet.delete_row",
            "sheet.delete_section",
            "sheet.evaluate",
            "sheet.read",
            "sheet.sections",
            "sheet.set_cell",
            "sheet.set_cells",
            "snap.at",
            "snap.set",
            "snap.settings",
            "stencil.add_quick",
            "stencil.close",
            "stencil.icon",
            "stencil.list",
            "stencil.open",
            "stencil.quick",
            "stencil.remove_quick",
            "stencil.reset",
            "stencil.search",
            "swimlane.create",
            "swimlane.info",
            "swimlane.insert",
            "swimlane.insert_phase",
            "swimlane.move",
            "swimlane.orientation",
            "swimlane.resize",
            "swimlane.rotate_label",
            "template.list",
            "text.block",
            "text.block_move",
            "text.block_resize",
            "text.block_rotate",
            "text.box",
            "text.field_choices",
            "text.field_sample",
            "text.format",
            "text.insert",
            "text.insert_field",
            "text.update_fields",
            "theme.apply",
            "theme.colors",
            "theme.list",
            "theme.preview",
            "theme.variant",
            "window.settings",
            "window.settings.set"
        ]
    );
    assert!(find_command("no.such").is_none());
}

/// The keys the params documentation of command `id` names, in order.
fn keys(id: &str) -> Vec<&'static str> {
    param_keys(find_command(id).unwrap().params).unwrap()
}

#[test]
fn the_params_docs_name_the_keys_of_the_page_file_and_batch_commands() {
    assert_eq!(keys("page.add"), ["name", "like", "background"]);
    assert_eq!(keys("page.duplicate"), ["id"]);
    // A page's type and background: the page is `page`, as in the page setup commands.
    assert_eq!(keys("page.type"), ["type", "page"]);
    assert_eq!(keys("page.background"), ["background", "page"]);
    assert_eq!(keys("page.rename"), ["id", "name"]);
    // Page setup: a page is `page`, left out the first that is not a background; the page list commands name theirs `id`.
    assert_eq!(keys("page.size"), ["preset", "width", "height", "page"]);
    assert_eq!(keys("page.orientation"), ["orientation", "page"]);
    assert_eq!(keys("page.scale"), ["preset", "paper", "paper_unit", "drawing", "drawing_unit", "page"]);
    assert_eq!(keys("page.paper"), ["preset", "kind", "orientation", "page"]);
    assert_eq!(keys("page.auto_size"), ["on", "page"]);
    assert_eq!(keys("page.setup"), ["page"]);
    assert_eq!(keys("page.delete"), ["id"]);
    assert_eq!(keys("page.move"), ["id", "to", "renumber"]);
    assert!(keys("page.presets").is_empty());
    assert_eq!(keys("command.batch"), ["commands", "label"]);
    assert_eq!(keys("file.open"), ["path", "edit_template"]);
    assert_eq!(keys("file.save_as"), ["path", "force", "drop_macros", "drop_objects"]);
    assert_eq!(keys("file.save"), ["force"]);
    assert_eq!(keys("file.check_compatibility"), ["path", "drop_macros", "drop_objects", "force"]);
    assert_eq!(keys("file.export"), ["path", "page", "dpi", "text", "transparent"]);
    assert_eq!(keys("file.print"), ["path", "page", "text", "color"]);
    assert_eq!(keys("file.print_preview"), ["page"]);
    assert_eq!(keys("file.new"), ["template"]);
    for id in ["app.version", "document.inspect", "file.status", "edit.undo", "edit.redo", "stencil.list"] {
        assert!(keys(id).is_empty(), "{id}");
    }
}

#[test]
fn the_params_docs_name_the_keys_of_the_shape_and_connector_commands() {
    // The editing commands keep the names of the M3 plan's table; `page` is optional on all but `shape.drop`.
    assert_eq!(keys("shape.drop"), ["master", "stencil", "page", "x", "y", "snap"]);
    assert_eq!(keys("shape.move"), ["ids", "dx", "dy", "page", "snap"]);
    assert_eq!(keys("shape.resize"), ["id", "ids", "x", "y", "width", "height", "page", "snap"]);
    assert_eq!(keys("shape.delete"), ["ids", "connectors", "page"]);
    assert_eq!(keys("shape.set_text"), ["id", "text", "start", "end", "page"]);
    assert_eq!(keys("connector.add"), ["from", "to", "page", "snap"]);
    assert_eq!(keys("connector.reconnect"), ["id", "end", "target", "page", "snap"]);
    assert_eq!(keys("connector.reverse_ends"), ["id", "ids", "page"]);
    // The routing styles with a direction and the route a connector has.
    assert_eq!(keys("connector.style"), ["style", "direction", "ids", "page"]);
    assert_eq!(keys("connector.path"), ["id", "page"]);
    assert_eq!(keys("page.routing"), ["page"]);
    assert_eq!(
        keys("page.set_routing"),
        [
            "style",
            "direction",
            "separate",
            "overlap",
            "appearance",
            "plow",
            "split",
            "between_shapes",
            "shape_size",
            "connector_to_connector",
            "connector_to_shape",
            "page"
        ]
    );
}

#[test]
fn the_params_docs_name_the_keys_of_the_arrange_and_format_commands() {
    // The arrange commands take the shapes as `ids`, and the first of them is the primary one for `shape.align`.
    // (row 7: `shape.rotate` takes `snap`, so its params doc names it, between `by` and `page`.)
    assert_eq!(keys("shape.rotate"), ["ids", "angle", "by", "about", "snap", "page"]);
    assert_eq!(keys("shape.flip"), ["ids", "axis", "page"]);
    assert_eq!(keys("shape.order"), ["ids", "to", "page"]);
    assert_eq!(keys("shape.align"), ["ids", "how", "page"]);
    // The format commands: the plan's table, `page` second.
    assert_eq!(
        keys("shape.format"),
        [
            "ids",
            "page",
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
            "fill_pattern",
            "fill_background",
            "line_cap",
            "compound"
        ]
    );
    // The gradient, the shadow and the Quick Styles.
    assert_eq!(keys("shape.gradient"), ["ids", "page", "target", "on", "kind", "origin", "direction", "angle", "rotate_with_shape", "stops"]);
    assert_eq!(keys("shape.shadow"), ["ids", "page", "on", "preset", "color", "transparency", "size", "blur", "angle", "distance"]);
    assert_eq!(keys("shape.quick_style"), ["ids", "page", "row", "column", "variant"]);
    assert_eq!(keys("shape.quick_styles"), ["ids", "page"]);
    assert_eq!(
        keys("text.format"),
        [
            "ids",
            "page",
            "start",
            "end",
            "font",
            "size",
            "bold",
            "italic",
            "underline",
            "strike",
            "color",
            "align",
            "valign",
            "bullet",
            "indent",
            "case",
            "direction",
            "vertical"
        ]
    );
    assert_eq!(keys("shape.formatting"), ["ids", "page", "start", "end"]);
    // The Insert tab's hyperlinks, fields and characters.
    assert_eq!(keys("insert.picture"), ["path", "x", "y", "width", "height", "page"]);
    assert_eq!(keys("hyperlink.set"), ["id", "links", "page"]);
    assert_eq!(keys("hyperlink.list"), ["id", "page"]);
    assert_eq!(keys("hyperlink.follow"), ["id", "link", "page"]);
    // The Data tab's Shape Data.
    assert_eq!(keys("shapedata.list"), ["id", "hidden", "page"]);
    assert_eq!(keys("shapedata.set"), ["id", "row", "value", "values", "page"]);
    assert_eq!(keys("shapedata.define"), ["id", "fields", "page"]);
    assert!(keys("shapedata.types").is_empty());
    assert_eq!(keys("text.insert_field"), ["id", "category", "field", "formula", "format", "at", "page"]);
    assert_eq!(keys("text.field_choices"), ["id", "page"]);
    assert_eq!(keys("text.field_sample"), ["id", "category", "field", "formula", "format", "page"]);
    assert_eq!(keys("text.update_fields"), ["id", "page"]);
    assert_eq!(keys("text.insert"), ["id", "text", "at", "page"]);
    // The Insert tab's callouts.
    assert!(keys("callout.styles").is_empty());
    assert_eq!(keys("callout.insert"), ["style", "ids", "x", "y", "text", "page"]);
    assert_eq!(keys("callout.attach"), ["id", "target", "page"]);
    assert_eq!(keys("callout.detach"), ["id", "x", "y", "page"]);
    assert_eq!(keys("callout.list"), ["id", "page"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_clipboard_group_and_stencil_commands() {
    // The clipboard commands and group and ungroup: the shapes are `ids`, and `edit.paste` takes the place of the copies.
    assert_eq!(keys("edit.copy"), ["ids", "page"]);
    assert_eq!(keys("edit.cut"), ["ids", "page"]);
    assert_eq!(keys("edit.paste"), ["page", "x", "y"]);
    assert_eq!(keys("shape.duplicate"), ["ids", "dx", "dy", "page"]);
    assert_eq!(keys("shape.group"), ["ids", "page"]);
    assert_eq!(keys("shape.ungroup"), ["ids", "page"]);
    // The stencil commands.
    assert_eq!(keys("stencil.open"), ["path", "id"]);
    assert_eq!(keys("stencil.close"), ["id"]);
    assert_eq!(keys("stencil.search"), ["text", "mode"]);
    assert_eq!(keys("stencil.quick"), ["stencil"]);
    assert_eq!(keys("stencil.add_quick"), ["stencil", "master", "at"]);
    assert_eq!(keys("stencil.remove_quick"), ["stencil", "master"]);
    assert_eq!(keys("stencil.reset"), ["stencil"]);
    assert_eq!(keys("stencil.icon"), ["stencil", "master"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_template_commands() {
    // File > New: a template is `template`, an id of `template.list` or the path of a template file; the list takes nothing.
    assert!(keys("template.list").is_empty());
}

#[test]
fn the_params_docs_name_the_keys_of_the_text_box_and_text_block_commands() {
    // The Text tool's commands: the box is placed by its centre as `shape.drop` places a shape; the block commands name one shape by `id`.
    assert_eq!(keys("text.box"), ["page", "x", "y", "width", "height", "text"]);
    // The drawing tools.
    assert_eq!(keys("shape.draw"), ["page", "tool", "points", "shift", "tolerance", "smoothing", "continue", "close"]);
    assert_eq!(keys("shape.close"), ["id", "page"]);
    // Select by Type, Area Select and Lasso Select.
    assert_eq!(keys("select.by_type"), ["types", "roles", "layers", "page"]);
    assert_eq!(keys("select.by_region"), ["polygon", "rect", "partial", "page"]);
    assert_eq!(keys("select.choices"), ["page"]);
    assert_eq!(keys("text.block"), ["id", "page"]);
    assert_eq!(keys("text.block_move"), ["id", "dx", "dy", "page"]);
    assert_eq!(keys("text.block_resize"), ["id", "x", "y", "width", "height", "page"]);
    assert_eq!(keys("text.block_rotate"), ["id", "angle", "by", "page"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_theme_commands() {
    // The Design tab's Themes and Variants galleries: a page is `page`, as in the page setup commands, and `scope` widens it to every page.
    assert_eq!(keys("theme.list"), ["page"]);
    assert_eq!(keys("theme.apply"), ["theme", "variant", "scope", "page"]);
    assert_eq!(keys("theme.variant"), ["variant", "colors", "styles", "scope", "page"]);
    // The palette of a page's theme and the preview of a theme or a variant on a copy.
    assert_eq!(keys("theme.colors"), ["page"]);
    assert_eq!(keys("theme.preview"), ["theme", "variant", "colors", "styles", "scope", "page", "ids"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_size_and_position_commands() {
    // The Size & Position window's commands: the shapes are `ids` where several can be given one thing, `id` for a line; `page` last.
    assert_eq!(keys("shape.pin"), ["ids", "at", "page"]);
    assert_eq!(keys("shape.place"), ["ids", "x", "y", "page", "snap"]);
    assert_eq!(keys("shape.size"), ["ids", "width", "height", "page"]);
    assert_eq!(keys("shape.set_end"), ["id", "end", "x", "y", "page"]);
    assert_eq!(keys("shape.set_length"), ["id", "length", "page"]);
    assert_eq!(keys("shape.placement"), ["ids", "page"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_behavior_commands() {
    // The Behavior dialog's Placement tab: the shapes are `ids`, each box of the tab a key, `page` last.
    assert_eq!(keys("shape.behavior"), ["ids", "page"]);
    assert_eq!(keys("shape.set_behavior"), ["ids", "placement", "route_through_x", "route_through_y", "do_not_move", "allow_on_top", "plow", "page"]);
}

#[test]
fn the_params_docs_name_the_keys_of_the_connection_point_commands() {
    // The shapes are `id` or `ids`, then the page and the minimum distance between points.
    assert_eq!(keys("shape.connection_edges"), ["id", "ids", "page", "min_gap"]);
    // The shapes, what to do, which edge (by number or by a place with its reach), then the page and the minimum distance.
    assert_eq!(keys("shape.connection_points"), ["id", "ids", "action", "edge", "at", "reach", "page", "min_gap"]);
}

#[test]
fn every_command_with_empty_params() {
    let table_ids: Vec<&str> = EMPTY_PARAMS.iter().map(|(id, _)| *id).collect();
    let registered: Vec<&str> = command_specs().iter().map(|c| c.id).collect();
    for id in &registered {
        assert!(table_ids.contains(id), "`{id}` is not in EMPTY_PARAMS: say what `{{}}` does to it");
    }
    for id in &table_ids {
        assert!(registered.contains(id), "`{id}` is in EMPTY_PARAMS but no such command is registered");
    }

    let mut failures = vec![];
    for (id, expected) in EMPTY_PARAMS {
        for params in [json!({}), Value::Null] {
            let mut s = rich_session();
            let before = snapshot(&s);
            let r = s.execute(id, &params);
            match (expected, &r) {
                (WithEmptyParams::Succeeds, Ok(_)) => {}
                (WithEmptyParams::NeedsParams, Err(EngineError::BadParams { .. }))
                | (WithEmptyParams::NeedsAFile, Err(EngineError::Failed { .. }))
                | (WithEmptyParams::NothingYet, Err(EngineError::Disabled { .. })) => {
                    if snapshot(&s) != before {
                        failures.push(format!("{id} {params}: failed but changed the session"));
                    }
                }
                _ => failures.push(format!("{id} {params}: expected {expected:?}, got {r:?}")),
            }
            if s.panics_caught() != 0 {
                failures.push(format!("{id} {params}: caught a panic"));
            }
            if let Err(e) = check_session(&s) {
                failures.push(format!("{id} {params}: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_command_without_a_document() {
    let mut failures = vec![];
    for c in command_specs() {
        let mut s = Session::new();
        let runs = RUNS_WITHOUT_A_DOCUMENT.contains(&c.id);
        let info = s.commands().into_iter().find(|i| i.id == c.id).unwrap();
        let r = s.execute(c.id, &json!({}));
        if NEEDS_PARAMS_WITHOUT_A_DOCUMENT.contains(&c.id) {
            // Enabled, and `{}` is refused for what it lacks: not a refusal for having no document.
            if !info.enabled || !matches!(r, Err(EngineError::BadParams { .. })) || snapshot(&s) != snapshot(&Session::new()) {
                failures.push(format!("{}: expected an enabled command that needs params and changes nothing, got {r:?} ({info:?})", c.id));
            }
            continue;
        }
        match (&r, runs) {
            (Ok(_), true) => {
                if !info.enabled {
                    failures.push(format!("{}: ran without a document but is listed as disabled", c.id));
                }
            }
            (Err(EngineError::Disabled { cmd, reason }), false) => {
                if cmd != c.id || info.enabled || info.disabled_reason.as_deref() != Some(reason.as_str()) {
                    failures.push(format!("{}: the error ({cmd}, {reason}) and the command list ({info:?}) disagree", c.id));
                }
                if snapshot(&s) != snapshot(&Session::new()) {
                    failures.push(format!("{}: refused but changed the session", c.id));
                }
            }
            _ => failures.push(format!("{}: {} to run without a document, got {r:?}", c.id, if runs { "expected" } else { "not expected" })),
        }
        if s.panics_caught() != 0 {
            failures.push(format!("{}: caught a panic", c.id));
        }
        if let Err(e) = check_session(&s) {
            failures.push(format!("{}: {e}", c.id));
        }
    }
    for id in RUNS_WITHOUT_A_DOCUMENT {
        assert!(find_command(id).is_some(), "`{id}` is in RUNS_WITHOUT_A_DOCUMENT but no such command is registered");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Text with control characters, as a hostile caller sends for an id or a key: it must never come back
/// raw in an error message.
const HOSTILE_TEXT: &[&str] = &["\u{1b}[2Jevil\nline", "tab\there", "nul\0byte", "\u{7f}del", "\u{85}next line", "\r\n\r\n", "\u{202e}rtl override"];

/// Hostile parameters for a command: not an object, an unknown key, then each documented key set to
/// each junk value, and all of them at once.
fn junk_params(doc: &str) -> Vec<Value> {
    let keys = param_keys(doc).unwrap();
    let junk = junk_values();
    let mut out = vec![Value::Null, json!([]), json!("str"), json!(42), json!({"unknownKey": 1}), json!({"": 1})];
    // Keys made of control characters: they come back in "unknown parameter" errors.
    for key in HOSTILE_TEXT {
        out.push(json!({ *key: 1 }));
    }
    for key in &keys {
        for v in &junk {
            out.push(json!({ *key: v.clone() }));
        }
    }
    if !keys.is_empty() {
        for v in &junk {
            out.push(Value::Object(keys.iter().map(|k| ((*k).to_owned(), v.clone())).collect()));
        }
    }
    out
}

/// Junk inside `command.batch`'s structure, which the plain junk values do not reach.
fn junk_batches() -> Vec<Value> {
    let step = |command: &str, params: Value| json!({"command": command, "params": params});
    vec![
        json!({"commands": [step("command.batch", json!({"commands": []}))]}),
        json!({"commands": [step("page.add", json!({})), step("command.batch", json!({"commands": [step("page.add", json!({}))]}))]}),
        json!({"commands": [{"command": 5}]}),
        json!({"commands": [{"command": "page.add", "params": [1]}]}),
        json!({"commands": [{"command": "page.add", "params": "x"}]}),
        json!({"commands": [{"command": "page.add", "extra": 1}]}),
        json!({"commands": [null, 1, "x", [], {}]}),
        json!({"commands": [step("page.rename", json!({"id": u64::MAX, "name": "x"}))]}),
        json!({"commands": [step("edit.undo", json!({})), step("file.new", json!({}))]}),
        json!({"commands": vec![step("app.version", json!({})); 10_000]}),
        json!({"commands": vec![step("page.add", json!({})); 300]}),
        json!({"commands": vec![step("page.add", json!({"name": "x".repeat(300)})); 3]}),
        // Control characters in a step's command, in a key of its params and in a key of the step.
        json!({"commands": [step(HOSTILE_TEXT[0], json!({}))]}),
        json!({"commands": [step("page.add", json!({}) ), step("page.add", json!({ HOSTILE_TEXT[0]: 1 }))]}),
        json!({"commands": [{"command": "page.add", HOSTILE_TEXT[3]: 1}]}),
        json!({"commands": [step(&format!("{}{}", HOSTILE_TEXT[0].repeat(50), "z".repeat(5000)), json!({}))]}),
    ]
}

#[test]
fn every_command_with_junk_params() {
    type Make = fn() -> Session;
    let states: [(&str, Make); 3] = [("no document", Session::new), ("a blank document", blank_session), ("history to undo and redo", rich_session)];
    let mut failures = vec![];
    let mut calls = 0usize;
    for (state, make) in states {
        for c in command_specs() {
            let mut cases = junk_params(c.params);
            if c.id == "command.batch" {
                cases.extend(junk_batches());
            }
            for params in cases {
                calls += 1;
                let mut s = make();
                let before = snapshot(&s);
                let r = s.execute(c.id, &params);
                let at = || format!("{} [{state}] {}", c.id, params.to_string().chars().take(120).collect::<String>());
                if let Err(e) = &r
                    && e.to_string().chars().any(char::is_control)
                {
                    failures.push(format!("{}: the error text holds a control character: {:?}", at(), e.to_string()));
                }
                match &r {
                    Err(e) if is_internal(e) => failures.push(format!("{}: PANIC caught: {e}", at())),
                    Err(EngineError::UnknownCommand(_)) => failures.push(format!("{}: the command was not found", at())),
                    Err(_) if snapshot(&s) != before => failures.push(format!("{}: failed but changed the session", at())),
                    Ok(_) if QUERIES.contains(&c.id) && snapshot(&s) != before => failures.push(format!("{}: a query changed the session", at())),
                    _ => {}
                }
                if s.panics_caught() != 0 {
                    failures.push(format!("{}: caught a panic", at()));
                }
                if let Err(e) = check_session(&s) {
                    failures.push(format!("{}: {e}", at()));
                }
            }
        }
    }
    assert!(calls > 500, "only {calls} calls: the sweep has gone thin");
    failures.sort();
    failures.dedup();
    assert!(
        failures.is_empty(),
        "{} failures (of {calls} calls):\n{}",
        failures.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The model a command made survives DiaCraft's own format (the design's section 10, "Commands"): each sample is run in its session, the drawing is saved as `.diacraft`
/// and opened in a session of its own, and the model after the reopen is the model saved, for a second save of it too. A command whose result the format could not hold would be a drawing that
/// is lost the first time it is saved.
#[test]
fn every_commands_result_survives_a_diacraft_save_and_reopen() {
    let dir = crate::native_common::Dir::new();
    let samples = undoable_samples();
    assert!(samples.len() > 40, "the sweep looks at the commands: {}", samples.len());
    for (k, (id, params)) in samples.into_iter().enumerate() {
        let mut s = session_with_history();
        for (setup_id, setup_params) in setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        run(&mut s, id, params.clone());
        let first = dir.path(&format!("c{k}.diacraft"));
        run(&mut s, "file.save_as", json!({ "path": first }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": first }));
        assert!(s.document() == again.document(), "after `{id}` {params}: the model read back is not the model saved");
        check_session(&again).unwrap_or_else(|e| panic!("after `{id}` {params}: {e}"));
        // The reopened drawing saves as the same file, and its second generation is the same model.
        let second = dir.path(&format!("d{k}.diacraft"));
        run(&mut again, "file.save_as", json!({ "path": second }));
        assert!(
            dir.read(&format!("c{k}.diacraft")) == dir.read(&format!("d{k}.diacraft")),
            "after `{id}` {params}: the save after a reopen is another file"
        );
    }
}

/// **The design's section 10, "Commands".** The result of every undoable command is saved as **both foreign formats** and read back through the real readers, and then
/// **across**: the VSDX file is opened and saved as an `.odg`, the `.odg` is opened and saved as a VSDX type. Each save answers the `carried` field, and **none is refused**: the engine's own
/// drawings hold nothing a model writer refuses **but a background page**, which an `.odg` the model writer makes does not hold (a master page that holds shapes: the design's short list of states the
/// target cannot represent, refused in words with nothing written). Every refusal is of that class and only of it.
#[test]
fn every_commands_result_saves_as_both_foreign_formats_and_reads_back() {
    let dir = crate::native_common::Dir::new();
    let samples = undoable_samples();
    assert!(samples.len() > 40, "the sweep looks at the commands: {}", samples.len());
    let made = |id: &str, params: &Value| {
        let mut s = session_with_history();
        for (setup_id, setup_params) in setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        run(&mut s, id, params.clone());
        s
    };
    let vsdx_ok = |name: &str, what: &str| {
        diacraft_vsdx::read(&dir.read(name), &diacraft_vsdx::Limits::default())
            .unwrap_or_else(|e| panic!("{what}: the VSDX file does not read: {e}"));
    };
    let odg_ok = |name: &str, what: &str| {
        diacraft_odg::read(&dir.read(name), &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{what}: the .odg does not read: {e}"));
    };
    let mut refused: Vec<String> = Vec::new();
    for (k, (id, params)) in samples.into_iter().enumerate() {
        let what = format!("after `{id}` {params}");
        // From the drawing to a VSDX type, and across to an `.odg`.
        let mut s = made(id, &params);
        run(&mut s, "file.save_as", json!({ "path": dir.path(&format!("v{k}.vsdx")) }));
        vsdx_ok(&format!("v{k}.vsdx"), &what);
        match s.execute("file.save_as", &json!({ "path": dir.path(&format!("v{k}-across.odg")) })) {
            Ok(out) => {
                assert!(out["carried"]["classes"].is_array(), "{what}: {out}");
                odg_ok(&format!("v{k}-across.odg"), &what);
            }
            Err(e) => refused.push(format!("{what}: VSDX to .odg: {e}")),
        }
        check_session(&s).unwrap_or_else(|e| panic!("{what}: {e}"));
        // From the drawing to an `.odg`, and across to a VSDX type.
        let mut t = made(id, &params);
        match t.execute("file.save_as", &json!({ "path": dir.path(&format!("o{k}.odg")) })) {
            Ok(_) => {
                odg_ok(&format!("o{k}.odg"), &what);
                match t.execute("file.save_as", &json!({ "path": dir.path(&format!("o{k}-across.vsdx")) })) {
                    Ok(out) => {
                        assert!(out["carried"]["classes"].is_array(), "{what}: {out}");
                        vsdx_ok(&format!("o{k}-across.vsdx"), &what);
                    }
                    Err(e) => refused.push(format!("{what}: .odg to VSDX: {e}")),
                }
            }
            Err(e) => refused.push(format!("{what}: to .odg: {e}")),
        }
        check_session(&t).unwrap_or_else(|e| panic!("{what}: {e}"));
    }
    // A refusal is for a background page, or for a position that holds a picture: an `.odg` has no place for the text of a picture frame, and the words say so.
    assert!(
        refused.iter().all(|r| {
            r.contains("Nothing was written")
                && (r.contains("a background page") || (r.contains("`orgchart.picture_") && r.contains("a text on a picture")))
        }),
        "{} saves were refused, and not every one for a background page or a position's picture, in words:\n{}",
        refused.len(),
        refused.join("\n")
    );
}

#[test]
fn every_command_undoes_exactly() {
    let samples = undoable_samples();
    let undoable: Vec<&str> = samples.iter().map(|(id, _)| *id).collect();

    // Every command is looked at: it is undoable (and checked below), a query, or excluded with a reason.
    for c in command_specs() {
        let kinds = [undoable.contains(&c.id), QUERIES.contains(&c.id), EXCLUDED.iter().any(|(id, _)| *id == c.id)];
        assert_eq!(
            kinds.iter().filter(|k| **k).count(),
            1,
            "command `{}` is {} of undoable_samples, QUERIES and EXCLUDED: a command that changes the document goes in undoable_samples \
             with params that succeed, one that only reads goes in QUERIES, and one that cannot be undone goes in EXCLUDED with the reason",
            c.id,
            if kinds.iter().any(|k| *k) { "in more than one" } else { "in none" },
        );
        assert_eq!(c.journal, !QUERIES.contains(&c.id), "`{}`: queries are not journaled, everything else is", c.id);
    }
    for id in QUERIES.iter().copied().chain(EXCLUDED.iter().map(|(id, _)| *id)).chain(undoable.iter().copied()) {
        assert!(find_command(id).is_some(), "`{id}` is listed in the undo sweep but no such command is registered");
    }
    for (id, reason) in EXCLUDED {
        assert!(reason.split_whitespace().count() >= 5, "`{id}` is excluded without a real reason: {reason:?}");
    }

    // A query changes nothing. One that asks about shapes is given the shapes the editing commands start from (1 and 2, and the connector 3).
    for id in QUERIES {
        // A query that needs params, or a file of the drawing's own, is run with them where its own tests are (`stencil_open.rs`, `save_check_before.rs`).
        if EMPTY_PARAMS.iter().any(|(i, expected)| i == id && matches!(expected, WithEmptyParams::NeedsParams | WithEmptyParams::NeedsAFile)) {
            continue;
        }
        let mut s = rich_session();
        for (setup_id, setup_params) in setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        let before = snapshot(&s);
        let params = if *id == "shape.formatting" { json!({"ids": [1, 2, 3]}) } else { json!({}) };
        run(&mut s, id, params);
        assert_eq!(snapshot(&s), before, "query `{id}` changed the session");
    }

    // An undoable command makes exactly one undo step, and one undo takes back exactly that.
    for (id, params) in samples {
        let mut s = session_with_history();
        for (setup_id, setup_params) in setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        let before = s.document().cloned().unwrap();
        let steps = s.history().unwrap().undo_len();

        // No `continue` on a failure: a sample that does not run proves nothing.
        s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: the sample params must succeed: {e}"));
        let after = s.document().cloned().unwrap();
        assert_ne!(after, before, "{id} {params}: the sample changed nothing, so undoing it proves nothing");
        assert_eq!(s.history().unwrap().undo_len(), steps + 1, "{id}: exactly one undo step");
        assert_eq!(s.history().unwrap().redo_len(), 0);

        run(&mut s, "edit.undo", json!({}));
        let undone = s.document().unwrap();
        assert_eq!(undone, &before, "{id} {params}: undo must restore the document exactly");
        assert!(
            undone.pages.iter().zip(&before.pages).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)),
            "{id}: undo must bring back the very pages that were there"
        );
        assert_eq!(s.history().unwrap().undo_len(), steps, "{id}");
        assert_eq!(s.history().unwrap().redo_len(), 1, "{id}");

        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.document(), Some(&after), "{id} {params}: redo must restore the edited document exactly");
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn hostile_command_ids_never_come_back_raw() {
    let mut failures = vec![];
    for id in HOSTILE_TEXT {
        let mut s = crate::blank_session();
        let r = s.execute(id, &json!({}));
        match &r {
            Err(EngineError::UnknownCommand(_)) => {}
            other => failures.push(format!("{id:?}: expected UnknownCommand, got {other:?}")),
        }
        if let Err(e) = r
            && e.to_string().chars().any(char::is_control)
        {
            failures.push(format!("{id:?}: the error text holds a control character: {:?}", e.to_string()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The params of a call that changes nothing, for the commands whose own sample changes something each time it is run (a move by an amount, a turn by an angle). Every other
/// sample is run again as it stands, which is a call that changes nothing when the command sets and does not add (a format, a size, a name, a connector's end), and one that
/// changes something (a page more, a shape more) when it adds.
const CHANGE_NOTHING: &[(&str, &str)] = &[
    ("shape.move", r#"{"ids": [1], "dx": 0, "dy": 0}"#),
    ("shape.rotate", r#"{"ids": [1], "by": 0}"#),
    ("text.block_move", r#"{"id": 1, "dx": 0, "dy": 0}"#),
    ("text.block_rotate", r#"{"id": 1, "by": 0}"#),
    ("command.batch", r#"{"commands": [{"command": "page.rename", "params": {"id": 1, "name": "First"}}], "label": "Nothing"}"#),
];

/// The keys of an answer that count (or say) what the call changed: a command that changed nothing answers 0 for each, `false`, or an empty list.
const COUNTS_OF_CHANGES: &[&str] = &[
    "moved",
    "resized",
    "styled",
    "formatted",
    "rotated",
    "flipped",
    "changed",
    "changed_pages",
    "written",
    "reset",
    "pinned",
    "sized",
    "renamed",
    "cells_rewritten",
    "shapes_linked",
];

/// The counts an answer gives of what a call changed that are not nothing, as `key: value`.
fn counted_changes(answer: &Value) -> Vec<String> {
    let Some(map) = answer.as_object() else { return Vec::new() };
    map.iter()
        .filter(|(key, value)| {
            COUNTS_OF_CHANGES.contains(&key.as_str())
                && match value {
                    Value::Number(n) => n.as_f64() != Some(0.0),
                    Value::Bool(b) => *b,
                    Value::Array(list) => !list.is_empty(),
                    _ => false,
                }
        })
        .map(|(key, value)| format!("{key}: {value}"))
        .collect()
}

/// What a session shows of its history and of its drawing's being changed: the steps, the revision, whether the drawing is unsaved.
fn history_marks(s: &Session) -> (usize, usize, u64, bool) {
    (s.history().map_or(0, |h| h.undo_len()), s.history().map_or(0, |h| h.redo_len()), s.revision(), s.is_dirty())
}

/// **An edit that leaves the drawing as it was is no undo step, moves no revision and does not make the drawing unsaved, whichever command made it** (a move by nothing and a
/// reconnect to the place the end is at made a step of the history on a drawing that was the same, and a call of the sweep's commands run twice did so all over). Each sample is run, and
/// then run again (or its call that changes nothing, `CHANGE_NOTHING`): when the second call leaves the document as it was it leaves the history, the revision and the unsaved mark as
/// they were, and when it changes the document it is one step and the revision moves. A call that changes nothing counts nothing in its answer (`moved` 0, `changed` false).
#[test]
fn a_call_that_leaves_the_drawing_as_it_was_is_no_step_no_revision_and_no_unsaved_mark() {
    let samples = undoable_samples();
    let mut unchanged: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (id, params) in samples {
        let mut s = session_with_history();
        for (setup_id, setup_params) in setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: the sample params must succeed: {e}"));
        let again = CHANGE_NOTHING.iter().find(|(c, _)| *c == id).map_or_else(|| params.clone(), |(_, p)| serde_json::from_str::<Value>(p).unwrap());
        let (document, marks) = (s.document().cloned().unwrap(), history_marks(&s));
        let Ok(answer) = s.execute(id, &again) else { continue };
        let changed = s.document() != Some(&document);
        let after = history_marks(&s);
        let what = format!("{id} {again} -> {answer}");
        if CHANGE_NOTHING.iter().any(|(c, _)| *c == id) && changed {
            failures.push(format!("{what}: the call that is to change nothing changed the document"));
        }
        if changed {
            if (after.0, after.1) != (marks.0 + 1, 0) || after.2 == marks.2 || !after.3 {
                failures.push(format!("{what}: a call that changed the document is one step and a revision: {marks:?} then {after:?}"));
            }
        } else {
            unchanged.push(id.to_owned());
            let counted = counted_changes(&answer);
            if !counted.is_empty() {
                failures.push(format!("{what}: the document is as it was, and the answer counts a change: {}", counted.join(", ")));
            }
            if after != marks {
                failures.push(format!(
                    "{what}: the document is as it was, and the history, the revision or the unsaved mark moved: {marks:?} then {after:?}"
                ));
            }
        }
        check_session(&s).unwrap_or_else(|e| panic!("{what}: {e}"));
    }
    assert!(unchanged.len() >= 60, "the sweep found only {} calls that change nothing: {unchanged:?}", unchanged.len());
    assert!(failures.is_empty(), "{} of {} calls:\n{}", failures.len(), unchanged.len(), failures.join("\n"));
}
