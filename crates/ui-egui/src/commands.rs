//! The window's own commands: the ones that change what the window shows and not the document.
//!
//! The engine's registry holds the commands that change the document. These belong to the window (its zoom, its folded
//! panels, which tab is open, which page it shows), so they live here and are reached the same ways: a ribbon button, the
//! control channel's `engine.execute`, a test. `engine.commands` lists both kinds. Each has an id, a label, a parameter
//! doc and an enabled check, as the engine's have, and a key it does not list is an error.
//!
//! The ribbon enables a control only if the command it names is in one of the two registries ([`known`]) and is enabled
//! now ([`DiacraftApp::command_enabled`]); a test walks the ribbon to hold it to that.

use diacraft_engine::doc::WindowFlags;
use diacraft_engine::draw::Op;
use diacraft_engine::{EngineError, Result, find_command, param_keys, shown};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::canvas::page_drawing::{DEFAULT_PAGE_INCHES, page_settings};
use crate::canvas::view::{MAX_ZOOM, MIN_ZOOM, View};
use crate::state::BackstagePage;

/// A command of the window.
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    /// What it takes, in the engine's format (`none`, or `name: description` entries joined by `;`).
    pub params: &'static str,
}

/// The window's commands, in the order they are listed.
pub const UI_COMMANDS: &[UiCommand] = &[
    UiCommand { id: "view.zoomIn", label: "Zoom In", menu: &["View"], shortcut: Some("Cmd+="), params: "none" },
    UiCommand { id: "view.zoomOut", label: "Zoom Out", menu: &["View"], shortcut: Some("Cmd+-"), params: "none" },
    UiCommand {
        id: "view.zoom",
        label: "Zoom",
        menu: &["View"],
        shortcut: None,
        params: "percent: number, the zoom in percent from 10 to 400; x: number, a screen x to zoom about (optional); y: number, a screen y to zoom about (optional, with x)",
    },
    UiCommand { id: "view.fitPage", label: "Fit Page to Window", menu: &["View"], shortcut: None, params: "none" },
    UiCommand {
        id: "window.find",
        label: "Find",
        menu: &["Home", "Editing", "Find"],
        shortcut: Some("Cmd+F"),
        params: "mode: text, find or replace, default find (or the mode the open dialog has): the dialog to open, a floating window that does not shade the drawing area, and an open one changes to it \
                 and keeps what was typed; what: text, optional, for Find what; with: text, optional, for Replace with; scope: text, optional, selection, page or all; \
                 in: array of text, shape_data, shape_name or user_cell, optional, what the Find dialog looks in (the Replace dialog replaces shape text); \
                 match_case: true or false, optional; whole_words: true or false, optional; \
                 action: text, optional, next, replace or replace_all: does what the button of that name does (Find Next runs `edit.find` from the match the dialog is at, then shows its page and \
                 selects its shape, Replace runs `edit.replace` of that match and goes on, Replace All runs `edit.replace` for every match as one undo step), and the answer's `find` says what it did; \
                 close: true to close the dialog",
    },
    UiCommand { id: "view.fitWidth", label: "Page Width", menu: &["View"], shortcut: None, params: "none" },
    UiCommand {
        id: "view.pageBreaks",
        label: "Page Breaks",
        menu: &["View", "Show"],
        shortcut: None,
        params: "on: true or false, and without it the box is toggled (View, Show, Page Breaks: the drawing's own setting, one `page_breaks.set` which a save keeps and which is no undo step, and on draws the lines where the page passes the printer paper)",
    },
    UiCommand {
        id: "window.relayout",
        label: "Re-Layout Page",
        menu: &["Design", "Layout"],
        shortcut: None,
        params: "style: text, optional, flowchart, hierarchy, compact_tree, radial or circular; direction: text, optional, the direction of the style (page.relayout); alignment: text, optional, for a hierarchy; \
                 spacing: number, optional, paper inches; connector_style: text, optional; appearance: text, optional; apply_routing: true or false, optional; enlarge_page: true or false, optional. \
                 It runs `page.relayout` for the shapes selected on the page on view, or for the whole page when none is, and what is selected stays selected",
    },
    UiCommand {
        id: "window.relayoutOptions",
        label: "More Layout Options",
        menu: &["Design", "Layout"],
        shortcut: None,
        params: "close: true to close the Configure Layout dialog without sending anything, and without it the dialog opens with what the page's layout cells say (Apply and OK run `page.relayout`)",
    },
    UiCommand { id: "window.autoSpace", label: "Auto Space", menu: &["Home", "Arrange", "Position"], shortcut: None, params: "none" },
    UiCommand {
        id: "window.spacingOptions",
        label: "Spacing Options",
        menu: &["Home", "Arrange", "Position"],
        shortcut: None,
        params: "close: true to close the Spacing Options dialog without sending anything, and without it the dialog opens with the page's two spaces (OK runs `page.spacing`)",
    },
    UiCommand {
        id: "arrange.moveOffPageBreaks",
        label: "Move off Page Breaks",
        menu: &["Home", "Arrange", "Position"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "arrange.avoidPageBreaks",
        label: "Avoid Page Breaks",
        menu: &["Home", "Arrange", "Position"],
        shortcut: None,
        params: "on: true or false, and without it the page's box is toggled (`page.avoid_page_breaks`)",
    },
    UiCommand {
        id: "window.zoomDialog",
        label: "Zoom",
        menu: &["View", "Zoom"],
        shortcut: None,
        params: "close: true to close the Zoom dialog without changing the zoom, and without it the dialog opens on Whole page (OK runs `view.zoom` for a listed percentage or a typed one, \
                 `view.fitWidth` for Page width and `view.fitPage` for Whole page, and Cancel changes nothing)",
    },
    UiCommand {
        id: "window.colorsDialog",
        label: "More Colors",
        menu: &["Home", "Shape Styles", "Fill"],
        shortcut: None,
        params: "target: text, the colour to choose: fill, line, color (the text), stop_color, line_stop_color, fill_background or shadow_color, which the palette's key names, and without it the dialog is \
                 not opened; close: true to close the Colors dialog without changing anything (OK sends one `format.shape`, `format.text`, `format.gradient` or `format.shadow` with the colour as #rrggbb, \
                 which is a fixed colour and is the newest of the Recent Colors, and Cancel and Escape change nothing)",
    },
    UiCommand {
        id: "view.pan",
        label: "Pan",
        menu: &["View"],
        shortcut: None,
        params: "dx: number, points to move the page to the right; dy: number, points to move it down",
    },
    UiCommand {
        id: "view.grid",
        label: "Grid",
        menu: &["View", "Show"],
        shortcut: None,
        params: "on: true or false, and without it the grid is toggled",
    },
    UiCommand {
        id: "view.rulers",
        label: "Ruler",
        menu: &["View", "Show"],
        shortcut: None,
        params: "on: true or false, and without it the rulers are toggled",
    },
    UiCommand {
        id: "view.guides",
        label: "Guides",
        menu: &["View", "Show"],
        shortcut: None,
        params: "on: true or false, and without it the guides are toggled",
    },
    UiCommand {
        id: "view.connectionPoints",
        label: "Connection Points",
        menu: &["View", "Visual Aids"],
        shortcut: None,
        params: "on: true or false, and without it the connection points are toggled (on, the connection points of the shapes in view are marked whatever the tool, \
                 and off, only while the Connector tool is on or a connector's end is dragged)",
    },
    UiCommand {
        id: "view.autoConnect",
        label: "AutoConnect",
        menu: &["View", "Visual Aids"],
        shortcut: None,
        params: "on: true or false, and without it the box is toggled (View, Visual Aids, AutoConnect: the drawing's own setting, one `autoconnect.set` which a save keeps, and on means \
                 the pointer resting on a shape shows its four arrows). Refused while the program-wide setting is off",
    },
    UiCommand {
        id: "window.enableAutoConnect",
        label: "Enable AutoConnect",
        menu: &["File", "Options", "Advanced"],
        shortcut: None,
        params: "on: true or false, and without it the setting is toggled (File, Options, Advanced, Enable AutoConnect: the program-wide setting, on at first and not saved, and off means no \
                 drawing shows arrows and the View box is greyed out)",
    },
    UiCommand {
        id: "window.snapGlue",
        label: "Snap & Glue",
        menu: &["View", "Visual Aids"],
        shortcut: None,
        params: "tab: text, General or Advanced: the Snap & Glue dialog opens on it with the document's settings (OK sends one `snap.set` with what was changed, the options and the six \
                 snap strengths); close: true to close the dialog without sending anything",
    },
    UiCommand {
        id: "window.layerProperties",
        label: "Layer Properties",
        menu: &["Home", "Editing", "Layers"],
        shortcut: None,
        params: "close: true to close the Layer Properties dialog without sending anything, and without it the dialog opens on the layers of the page on view (OK and Apply send one `command.batch` \
                 of `layer.delete`, `layer.set` and `layer.add` for what was changed, and Cancel nothing)",
    },
    UiCommand {
        id: "window.defineShapeData",
        label: "Define Shape Data",
        menu: &["Data", "Show/Hide"],
        shortcut: None,
        params: "close: true to close the Define Shape Data dialog without sending anything, and without it the dialog opens on the fields of the one shape selected, hidden ones too (OK sends one \
                 `shapedata.define` with the whole list, and Cancel nothing). The shape's right-click menu, Data, Define Shape Data, and the Shape Data window's own menu, open it",
    },
    UiCommand {
        id: "window.askShapeData",
        label: "Ask for Shape Data",
        menu: &["Data", "Show/Hide"],
        shortcut: None,
        params: "id: whole number, the shape (not needed with close); page: whole number, optional, the id of the page (default: the page on view); close: true to close the card without sending anything. \
                 Opens the card a drop asks, which lists the fields of the shape that have Ask on drop with an editor of each type (OK sends one `shapedata.set` of the values changed, and Cancel nothing). \
                 It opens nothing for a shape none of whose fields asks, and the Shapes window runs it after each drop",
    },
    UiCommand {
        id: "window.assignLayer",
        label: "Assign to Layer",
        menu: &["Home", "Editing", "Layers"],
        shortcut: None,
        params: "close: true to close the Assign to Layer dialog without sending anything, and without it the dialog opens for the shapes selected, each layer ticked, clear or mixed by how many of them \
                 are on it (OK sends one `command.batch` of `layer.add` and `layer.assign` for what was changed, and Cancel nothing)",
    },
    UiCommand {
        id: "window.containerAdd",
        label: "Add Container",
        menu: &["Insert", "Diagram Parts", "Container"],
        shortcut: None,
        params: "style: text, optional, one of the ids container.styles lists (default strip): the container goes round the shapes selected and holds them, or with none selected an empty one is dropped in the \
                 middle of the view, and the new container is selected",
    },
    UiCommand {
        id: "window.containerDo",
        label: "Container",
        menu: &["Container Format"],
        shortcut: None,
        params: "op: text, what to do to the containers selected (add_member: to the shapes selected): style, heading, margin, fit, auto_resize, lock, disband, select_contents or add_member; \
                 value: text, optional, what the operation takes: a style id (style), top or right or bottom or left or rotate (heading), none or narrow or medium or wide or inches (margin), \
                 none or expand or fit (auto_resize)",
    },
    UiCommand {
        id: "window.swimlaneDo",
        label: "Swimlanes",
        menu: &["Cross-Functional Flowchart"],
        shortcut: None,
        params: "op: text, what to do to the lane, the phase or the structure selected, or to the page's first structure with none selected: insert (a swimlane before or after the lane selected, or at the end), \
                 separator (splits the phase selected, or the last), orientation, rotate_label, earlier or later (the lane selected one place along its list); \
                 value: text, optional, before or after (insert) or horizontal or vertical (orientation), and the new lane or phase is selected",
    },
    UiCommand {
        id: "window.orgChartDo",
        label: "Org Chart",
        menu: &["Org Chart"],
        shortcut: None,
        params: "op: text, what the Org Chart tab does: layout, relayout, arrange, picture, picture_delete or picture_hide; \
                 value: text, optional, what the operation takes: one of the six layouts of orgchart.layout (layout), or horizontal, vertical_right, vertical_left or inherit (arrange, to the position selected); \
                 path: text, optional, the picture file (picture, to the position selected: without it the file chooser is shown, as Insert, Pictures does). The page is the one on view",
    },
    UiCommand {
        id: "window.ribbon",
        label: "Collapse the Ribbon",
        menu: &["Window"],
        shortcut: None,
        params: "collapsed: true or false, and without it the ribbon is toggled",
    },
    UiCommand {
        id: "window.shapes",
        label: "Shapes Window",
        menu: &["View", "Task Panes"],
        shortcut: None,
        params: "shown: true or false, and without it the Shapes window is toggled",
    },
    UiCommand {
        id: "window.stencil",
        label: "Show a Stencil",
        menu: &["View", "Task Panes"],
        shortcut: None,
        params: "stencil: text, the id or the name of a stencil in `stencil.list`: the Shapes window shows its masters",
    },
    UiCommand {
        id: "window.shapeFilter",
        label: "Filter this stencil",
        menu: &["Window", "Shapes"],
        shortcut: None,
        params: "query: text, case-insensitive substring of current stencil master names (scan at most 4096 input Unicode characters, CR/LF become spaces, other controls are removed, then cap at 128 Unicode characters)",
    },
    UiCommand {
        id: "shapes.search",
        label: "Search for Shapes",
        menu: &["Home", "Shapes", "More Shapes"],
        shortcut: None,
        params: "text: text, optional, the words to look for in the names and search words of the masters of every stencil (at most 128 characters, whole words, any case): the results are shown as the stencil `search`, each stencil's matches under its name, and blank text clears the search and shows the stencil that was shown before it (without `text`, the words of the last search are searched again); mode: text, optional, `all` (the default, a master must have every word) or `any` (one word is enough)",
    },
    UiCommand {
        id: "shapes.openStencil",
        label: "Open Stencil",
        menu: &["Home", "Shapes", "More Shapes"],
        shortcut: None,
        params: "path: text, optional, the stencil file (.diacraft-stencil, .vssx or .vssm) to open, and without it the file dialog is shown (the stencil is then the one the Shapes window shows)",
    },
    UiCommand {
        id: "shapes.closeStencil",
        label: "Close Stencil",
        menu: &["Home", "Shapes", "More Shapes"],
        shortcut: None,
        params: "id: text, the id of a stencil opened from a file (`builtin` false in `stencil.list`)",
    },
    UiCommand {
        id: "window.tab",
        label: "Open a Ribbon Tab",
        menu: &["Window"],
        shortcut: None,
        params: "tab: text, the label of the tab (Home, Insert, ...)",
    },
    UiCommand { id: "window.theme", label: "Theme", menu: &["Window"], shortcut: None, params: "theme: text, light or dark" },
    UiCommand {
        id: "window.backstage",
        label: "File View",
        menu: &["File"],
        shortcut: None,
        params: "page: text, Home, New or Print to open the File view on it, or close to leave it",
    },
    UiCommand {
        id: "window.templateDetail",
        label: "Template Detail",
        menu: &["File", "New"],
        shortcut: None,
        params: "template: text, optional, the id of a template in template.list: the File view opens on New with the detail of that template in place of the gallery (the page a press on its \
                 tile shows: Create makes the drawing and Close goes back); close: true to go back to the gallery",
    },
    UiCommand {
        id: "print.set",
        label: "Print Settings",
        menu: &["File", "Print"],
        shortcut: None,
        params: "printer: text, the name of a printer in the window state's print.printers, or `pdf` to save what would be printed as a file; \
                 copies: whole number, 1 to 999; collate: true or false; two_sided: text, default, off, long or short; \
                 color: text, color or grayscale; pages: text, all or current; \
                 sheet: whole number, the sheet to show in the preview, from 1; zoom: number of percent from 25 to 400, or the text `fit`",
    },
    UiCommand {
        id: "print.start",
        label: "Print",
        menu: &["File", "Print"],
        shortcut: None,
        params: "path: text, the file to write when the destination is a PDF a program names it, and the window opens no dialog for it. Leave it out when the destination is a printer",
    },
    UiCommand {
        id: "window.developer",
        label: "Show the Developer Tab",
        menu: &["Window"],
        shortcut: None,
        params: "on: true or false, and without it the tab is toggled",
    },
    UiCommand { id: "page.select", label: "Go to Page", menu: &["Page"], shortcut: None, params: "id: whole number, the id of the page to show" },
    UiCommand { id: "tool.pointer", label: "Pointer Tool", menu: &["Home", "Tools"], shortcut: Some("Cmd+1"), params: "none" },
    UiCommand { id: "tool.connector", label: "Connector Tool", menu: &["Home", "Tools"], shortcut: Some("Cmd+3"), params: "none" },
    UiCommand {
        id: "window.pane",
        label: "Task Pane",
        menu: &["View", "Show", "Task Panes"],
        shortcut: None,
        params: "pane: text, size_position, format_shape, pan_zoom, drawing_explorer, shape_data or comments (or the pane's title, Size & Position and so on): the pane at the right of the drawing area; \
                 shown: true to open it, false to close it, and without it the pane is toggled",
    },
    UiCommand {
        id: "window.newComment",
        label: "New Comment",
        menu: &["Review", "Comments"],
        shortcut: None,
        params: "id: whole number, optional, the shape the comment is for (default: the shape selected when one is, else the page); page: whole number, optional, the id of the page (default: the page on view). \
                 Opens the Comments pane with a box for the comment, and the comment is made by `comment.add` when it is posted",
    },
    UiCommand {
        id: "window.showComment",
        label: "Show Comment",
        menu: &["Review", "Comments"],
        shortcut: None,
        params: "comment: whole number, the comment to show: its page is shown, its shape selected, its card opened in the Comments pane",
    },
    UiCommand {
        id: "window.stepComment",
        label: "Step Through Comments",
        menu: &["Review", "Comments"],
        shortcut: None,
        params: "forward: true for the next comment, false for the previous one, from the comment that is open (`comment.next` and `comment.previous` say how they are ordered)",
    },
    UiCommand {
        id: "window.paneDock",
        label: "Dock a Pane",
        menu: &["View", "Show", "Task Panes"],
        shortcut: None,
        params: "pane: text, the pane to put in a space (it is opened if it is not open); \
                 with: text, optional, another pane that is open: the pane shares its space, with tabs along the bottom, and without it the pane takes a space of its own at the bottom of the stack",
    },
    UiCommand {
        id: "window.paneTab",
        label: "Show a Pane's Tab",
        menu: &["View", "Show", "Task Panes"],
        shortcut: None,
        params: "pane: text, an open pane that shares a space with others: its tab comes to the top of the space",
    },
    UiCommand {
        id: "window.paneMove",
        label: "Move a Pane's Space",
        menu: &["View", "Show", "Task Panes"],
        shortcut: None,
        params: "pane: text, an open pane; up: true to move its space one place up the stack, false to move it one place down",
    },
    UiCommand {
        id: "window.explorerBranch",
        label: "Fold or Unfold a Branch of Drawing Explorer",
        menu: &["View", "Show", "Task Panes"],
        shortcut: None,
        params: "branch: text, foreground_pages, background_pages, styles, masters, fill_patterns, line_patterns or line_ends (or its title); \
                 open: true to unfold it, false to fold it, and without it the branch is toggled",
    },
    UiCommand { id: "tool.area_select", label: "Area Select", menu: &["Home", "Editing", "Select"], shortcut: None, params: "none" },
    UiCommand { id: "tool.lasso_select", label: "Lasso Select", menu: &["Home", "Editing", "Select"], shortcut: None, params: "none" },
    UiCommand {
        id: "window.selectByType",
        label: "Select by Type",
        menu: &["Home", "Editing", "Select"],
        shortcut: None,
        params: "close: true to close the dialog without selecting, and without it the Select by Type dialog opens (the seven types, the two roles and the layers of the page on view, each ticked or not: \
                 OK is quiet until something is ticked and then sends one `edit.selectByType` with what is ticked, Cancel and Escape select nothing)",
    },
    UiCommand {
        id: "edit.selectByType",
        label: "Select by Type",
        menu: &["Home", "Editing", "Select"],
        shortcut: None,
        params: "types: array of words, optional, any of shape, group, guide, ole, metafile, bitmap and ink: the shapes of any of these types are selected; \
                 roles: array of words, optional, connector and container; \
                 layers: array of layer names or numbers, optional (the numbers being the IX of the page's layers). \
                 The shapes of the page on view that the engine's `select.by_type` takes for them are selected, with no dialog, and at least one list must be given",
    },
    UiCommand {
        id: "edit.selectRegion",
        label: "Select in a Region",
        menu: &["Home", "Editing", "Select"],
        shortcut: None,
        params: "polygon: array of [x, y] pairs, inches from the page's left and bottom edges, the region, the last point joining the first; \
                 rect: array of four numbers [x0, y0, x1, y1], inches, instead of polygon; \
                 partial: true or false, optional (default: the window's option, `window.selectPartially`): also the shapes the region touches and not only those wholly inside it; \
                 add: true to keep what is selected and add the shapes after it. \
                 The shapes of the page on view that the engine's `select.by_region` takes are selected, as a net or a loop with the pointer does",
    },
    UiCommand {
        id: "window.selectPartially",
        label: "Select shapes partially within area",
        menu: &["File", "Options", "Advanced"],
        shortcut: None,
        params: "on: true or false, and without it the setting is toggled (File, Options, Advanced, Select shapes partially within area: the program-wide setting, off at first and not saved, \
                 and on means that a net, Area Select and Lasso Select also select the shapes they touch)",
    },
    // The Insert tab.
    UiCommand {
        id: "window.insertPicture",
        label: "Pictures",
        menu: &["Insert", "Illustrations"],
        shortcut: None,
        params: "path: text, optional, the picture file (PNG, JPEG, GIF, BMP, TIFF, EMF or WMF), which `insert.picture` puts on the page on view and which becomes the selection, with no dialog \
                 (without it the desktop's file chooser is shown, and the file chosen is inserted the same way)",
    },
    UiCommand {
        id: "window.callout",
        label: "Callout",
        menu: &["Insert", "Diagram Parts"],
        shortcut: None,
        params: "style: text, optional, the id of a tile of callout.styles (default: the first): adds a callout of that style for each shape selected on the page on view, tied to it, or, with no shape \
                 selected, one tied to nothing at the middle of the drawing area (`callout.insert`), and the callouts made become the selection",
    },
    UiCommand {
        id: "window.hyperlinks",
        label: "Link",
        menu: &["Insert", "Links"],
        shortcut: Some("Cmd+K"),
        params: "close: true to close the Hyperlinks dialog without sending anything, and without it the dialog opens on the hyperlinks of the one shape selected (OK sends one `hyperlink.set` with the list, \
                 and Cancel nothing)",
    },
    UiCommand {
        id: "window.followLink",
        label: "Follow Hyperlink",
        menu: &["Insert", "Links"],
        shortcut: None,
        params: "id: whole number, the shape whose link is followed, as a Ctrl+click on it does: `hyperlink.follow` says where it goes, a link of this drawing shows its page (and selects its shape) and a web, \
                 FTP or mail address is handed to the system with egui's OpenUrl by the next frame",
    },
    UiCommand {
        id: "window.field",
        label: "Field",
        menu: &["Insert", "Text"],
        shortcut: None,
        params: "close: true to close the Field dialog without sending anything, and without it the dialog opens for the text box that is open (the field goes where the caret is) or the one shape selected \
                 (the field goes at the end of its text), and OK sends one `text.insert_field`",
    },
    UiCommand {
        id: "window.symbols",
        label: "More Symbols",
        menu: &["Insert", "Text"],
        shortcut: None,
        params: "close: true to close the Symbol dialog without inserting, and without it the dialog opens for the text box that is open or the one shape selected (Insert sends `text.insert`, or \
                 `shape.set_text` over the characters selected in the box)",
    },
    UiCommand {
        id: "window.symbol",
        label: "Symbol",
        menu: &["Insert", "Text"],
        shortcut: None,
        params: "text: text, the characters to put in, with no dialog, where the caret of the open text box is (over the characters selected in it) or at the end of the text of the one shape selected",
    },
    UiCommand { id: "tool.text", label: "Text Tool", menu: &["Home", "Tools"], shortcut: Some("Cmd+2"), params: "none" },
    UiCommand { id: "tool.text_block", label: "Text Block Tool", menu: &["Home", "Tools"], shortcut: Some("Cmd+Shift+4"), params: "none" },
    // The six drawing tools: Home, Tools, Drawing Tools, with the keys.
    UiCommand { id: "tool.rectangle", label: "Rectangle Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+8"), params: "none" },
    UiCommand { id: "tool.ellipse", label: "Ellipse Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+9"), params: "none" },
    UiCommand { id: "tool.line", label: "Line Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+6"), params: "none" },
    UiCommand { id: "tool.freeform", label: "Freeform Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+5"), params: "none" },
    UiCommand { id: "tool.arc", label: "Arc Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+7"), params: "none" },
    UiCommand { id: "tool.pencil", label: "Pencil Tool", menu: &["Home", "Tools", "Drawing Tools"], shortcut: Some("Cmd+4"), params: "none" },
    UiCommand { id: "draw.closeShape", label: "Close Shape", menu: &["Draw", "Create"], shortcut: None, params: "none" },
    UiCommand {
        id: "window.freeformSmoothing",
        label: "Freeform Smoothing",
        menu: &["File", "Options", "Advanced"],
        shortcut: None,
        params: "percent: number, 0 to 100, how much a stroke of the Freeform tool is smoothed before its curve is fitted (the window's own setting, 40 at first and not saved, and only read when it is left out)",
    },
    UiCommand {
        id: "format.rotateText",
        label: "Rotate Text",
        menu: &["Home", "Paragraph"],
        shortcut: None,
        params: "by: number, degrees on the page, counterclockwise: the text block of every shape selected is turned by this much about its pin (90 is the Rotate Text button)",
    },
    UiCommand {
        id: "window.pageSetup",
        label: "Page Setup",
        menu: &["Design", "Page Setup"],
        shortcut: None,
        params: "tab: text, Print Setup, Page Size, Drawing Scale or Page Properties: the Page Setup dialog opens on it for the page on view (the dialog changes nothing until its OK); \
                 page: whole number, optional, the id of the page to set up instead of the page on view; \
                 close: true to close the dialog without sending anything",
    },
    UiCommand {
        id: "window.textDialog",
        label: "Text",
        menu: &["Home", "Font"],
        shortcut: None,
        params: "tab: text, Font, Paragraph or Text Block: the Text dialog opens on it for the shapes selected, or for the characters selected in the text box that is open (OK sends one `text.format` with what was changed); \
                 close: true to close the dialog without sending anything",
    },
    UiCommand {
        id: "window.reorderPages",
        label: "Reorder Pages",
        menu: &["Page"],
        shortcut: Some("Cmd+Alt+P"),
        params: "close: true to close the Reorder Pages dialog, and without it the dialog opens",
    },
    UiCommand {
        id: "edit.select",
        label: "Select Shapes",
        menu: &["Home", "Editing"],
        shortcut: None,
        params: "ids: array of whole numbers, the shapes of the page shown to select (an empty array clears the selection)",
    },
    UiCommand { id: "edit.selectAll", label: "Select All", menu: &["Home", "Editing"], shortcut: Some("Cmd+A"), params: "none" },
    UiCommand {
        id: "format.shape",
        label: "Format Selected Shapes",
        menu: &["Home", "Shape Styles"],
        shortcut: None,
        params: "fill: text, optional, the fill as a colour #RRGGBB or none; \
                 line: text, optional, the line as a colour #RRGGBB or none; \
                 weight: number, optional, the thickness of the line in points on the paper; \
                 pattern: whole number, optional, the line pattern from 0 (no line) to 23; \
                 begin_arrow: whole number, optional, the line end at the start of the line from 0 to 45; \
                 end_arrow: whole number, optional, the line end at the end of the line from 0 to 45; \
                 begin_arrow_size: whole number, optional, how big the line end at the start is, from 0 (very small) to 6 (colossal), 2 being medium; \
                 end_arrow_size: whole number, optional, how big the line end at the end is, from 0 (very small) to 6 (colossal), 2 being medium; \
                 rounding: number, optional, the radius that rounds the corners in inches; \
                 fill_transparency: number, optional, how see-through the fill is, from 0 to 100 percent; \
                 line_transparency: number, optional, how see-through the line is, from 0 to 100 percent; \
                 fill_pattern: whole number, optional, the fill pattern from 1 (solid) to 24 (hatches and stipples); \
                 fill_background: text, optional, the colour between the marks of a pattern as #RRGGBB; \
                 line_cap: text, optional, round, flat or square; \
                 compound: text, optional, single, double, thick_thin, thin_thick or triple",
    },
    UiCommand {
        id: "format.gradient",
        label: "Gradient Fill",
        menu: &["Home", "Shape Styles"],
        shortcut: None,
        params: "target: text, optional, fill (the default) or line: which gradient is set; \
                 on: boolean, optional, false to turn the gradient off, and anything else given turns it on; \
                 kind: text, optional, linear, radial, rectangular or path, with origin; \
                 origin: text, optional, where a radial or rectangular gradient starts: center, top_left, top_right, bottom_left or bottom_right, and for a radial one also top or bottom; \
                 direction: whole number, optional, from 0 to 13: the layout and its origin as FillGradientDir holds them; \
                 angle: number, optional, degrees clockwise from the right as seen on the page, for a linear gradient; \
                 rotate_with_shape: boolean, optional, whether a fill's gradient turns with the shape; \
                 stops: array, optional, from 2 to 10 objects {position: from 0 to 1, color: #RRGGBB, transparency: from 0 to 100 percent}, which replace the stops the shape has; \
                 stop: whole number, optional, the stop edited by stop_color, stop_position, stop_transparency or remove_stop (the one the Format Shape pane has selected when it is left out); \
                 stop_color: text, optional, the colour of that stop as #RRGGBB; \
                 line_stop_color: text, optional, the same for the line's gradient; \
                 stop_position: number, optional, its place from 0 to 1; \
                 stop_transparency: number, optional, its transparency from 0 to 100 percent; \
                 add_stop: boolean, optional, true to add a stop in the widest gap, in the colour between its neighbours; \
                 remove_stop: boolean, optional, true to take that stop away",
    },
    UiCommand {
        id: "format.shadow",
        label: "Shadow",
        menu: &["Home", "Shape Styles"],
        shortcut: None,
        params: "on: boolean, optional, false to take the shadow away, and anything else given turns it on; \
                 preset: text, optional, none, bottom_right, bottom, bottom_left, right, center, left, top_right, top or top_left; \
                 color: text, optional, the colour of the shadow as #RRGGBB; \
                 shadow_color: text, optional, the same (what the palette of the Format Shape pane sends); \
                 transparency: number, optional, from 0 to 100 percent; \
                 size: number, optional, in percent of the shape, from 1 to 1000; \
                 blur: number, optional, in points on the paper, from 0 to 1000; \
                 angle: number, optional, degrees clockwise from the right as seen on the page; \
                 distance: number, optional, in points on the paper, from 0 to 1000",
    },
    UiCommand {
        id: "format.quickStyle",
        label: "Quick Styles",
        menu: &["Home", "Shape Styles"],
        shortcut: None,
        params: "row: whole number, optional, from 1 to 6: the row of the Theme Styles in the gallery, with column; \
                 column: whole number, optional, from 1 to 7: the column of the Theme Styles, with row; \
                 variant: whole number, optional, from 0 to 3: one of the four Variant Styles instead",
    },
    UiCommand {
        id: "format.options",
        label: "Format Options",
        menu: &["Home", "Shape Styles"],
        shortcut: None,
        params: "tab: text, optional, Fill & Line (the default) or Effects: the Format Shape pane opens on it for the shapes selected (Fill Options, Line Options and More Arrows are this)",
    },
    UiCommand {
        id: "format.painter",
        label: "Format Painter",
        menu: &["Home", "Clipboard"],
        shortcut: Some("Cmd+Shift+P"),
        params: "on: true to pick up the formatting of the shape selected (with two shapes selected or more, the first is the one copied and the others are painted at once), false to put the painter down, and without it the painter is toggled; \
                 keep: true to keep the painter on after it paints, until Escape (a double click on the button); \
                 to: array of whole numbers, optional, the shapes of the page shown to paint with what the painter holds, as a click on each would (the painter must be on)",
    },
    UiCommand {
        id: "format.text",
        label: "Format Selected Text",
        menu: &["Home", "Font"],
        shortcut: None,
        params: "font: text, optional, the name of the face; \
                 size: number, optional, the size in points; \
                 bold: boolean, optional; \
                 italic: boolean, optional; \
                 underline: boolean, optional; \
                 strike: boolean, optional; \
                 color: text, optional, the colour of the text as #RRGGBB; \
                 align: text, optional, left, center, right, justify or distributed; \
                 valign: text, optional, top, middle or bottom; \
                 bullet: whole number, optional, 0 for no bullet and 1 to 7 for the marks of the table; \
                 indent: text, optional, increase or decrease, a quarter inch to the next multiple; \
                 case: text, optional, normal, all_caps, initial_caps or small_caps; \
                 direction: text, optional, ltr or rtl, the reading direction of the paragraphs; \
                 vertical: boolean, optional, true to run the text down the block and false across",
    },
    UiCommand {
        id: "format.toggle",
        label: "Bold, Italic, Underline, Strikethrough, Bullets, Vertical Text, Caps",
        menu: &["Home", "Font"],
        shortcut: None,
        params: "what: text, bold, italic, underline, strike, bullets, vertical, all_caps or small_caps: it is turned off when every shape selected has it and on otherwise (bullets is the first bullet or none, and the two caps are their case or normal)",
    },
    UiCommand {
        id: "format.size",
        label: "Increase or Decrease Font Size",
        menu: &["Home", "Font"],
        shortcut: None,
        params: "step: text, up or down: the next size of the list above or below the size of the shape selected first",
    },
    UiCommand {
        id: "arrange.rotate",
        label: "Rotate Selected Shapes",
        menu: &["Home", "Arrange", "Position"],
        shortcut: None,
        params: "by: number, degrees counterclockwise: 90 is Rotate Left and -90 is Rotate Right",
    },
    UiCommand {
        id: "arrange.flip",
        label: "Flip Selected Shapes",
        menu: &["Home", "Arrange", "Position"],
        shortcut: None,
        params: "axis: text, horizontal (left to right) or vertical (top to bottom)",
    },
    UiCommand {
        id: "arrange.order",
        label: "Change the Drawing Order of Selected Shapes",
        menu: &["Home", "Arrange"],
        shortcut: None,
        params: "to: text, front, back, forward or backward",
    },
    UiCommand {
        id: "arrange.align",
        label: "Align Selected Shapes",
        menu: &["Home", "Arrange"],
        shortcut: None,
        params: "how: text, left, center, right, top, middle or bottom: the shapes selected after the first move to the first",
    },
    UiCommand { id: "clipboard.copy", label: "Copy", menu: &["Home", "Clipboard"], shortcut: None, params: "none" },
    UiCommand { id: "clipboard.cut", label: "Cut", menu: &["Home", "Clipboard"], shortcut: None, params: "none" },
    UiCommand {
        id: "clipboard.paste",
        label: "Paste",
        menu: &["Home", "Clipboard"],
        shortcut: None,
        params: "x: number, optional, the paper inch of the page where the middle of the copies goes; y: number, optional, the same up the page (give both or neither: with neither the copies land a step right and down)",
    },
    UiCommand {
        id: "clipboard.duplicate",
        label: "Duplicate",
        menu: &["Home", "Clipboard"],
        shortcut: None,
        params: "dx: number, optional, inches to the right of the originals, 0.25 by default; dy: number, optional, inches up from the originals, -0.25 by default",
    },
    UiCommand {
        id: "arrange.changeShape",
        label: "Change Shape",
        menu: &["Home", "Editing"],
        shortcut: None,
        params: "master: text, the id or the name of the master the selected shapes are changed to (`shape.change`); stencil: text, optional, the id of the stencil it is in (the built-in stencils are searched when it is left out)",
    },
    UiCommand { id: "arrange.group", label: "Group", menu: &["Home", "Arrange", "Group"], shortcut: None, params: "none" },
    UiCommand { id: "arrange.ungroup", label: "Ungroup", menu: &["Home", "Arrange", "Group"], shortcut: None, params: "none" },
    UiCommand {
        id: "operations.union",
        label: "Union the Selected Shapes",
        menu: &["Developer", "Shape Design", "Operations"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "operations.combine",
        label: "Combine the Selected Shapes",
        menu: &["Developer", "Shape Design", "Operations"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "operations.fragment",
        label: "Fragment the Selected Shapes",
        menu: &["Developer", "Shape Design", "Operations"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "operations.intersect",
        label: "Intersect the Selected Shapes",
        menu: &["Developer", "Shape Design", "Operations"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "operations.subtract",
        label: "Subtract the Selected Shapes",
        menu: &["Developer", "Shape Design", "Operations"],
        shortcut: None,
        params: "none",
    },
    UiCommand {
        id: "window.options",
        label: "Options",
        menu: &["File"],
        shortcut: None,
        params: "tab: text, Customize Ribbon or Advanced: the Options dialog opens on it with the Developer tab's box and the box Select shapes partially within area as the window has them \
                 (OK sends `window.developer` and `window.selectPartially` for the boxes that were changed); \
                 close: true to close the dialog without changing anything",
    },
    UiCommand {
        id: "app.about",
        label: "About DiaCraft",
        menu: &["File"],
        shortcut: None,
        params: "close: true to close the panel at once, and without it the panel opens (the octopus cover over the whole popup, with the version, the git release, the year and the licence with a link to \
                 each of its two pages, and a link to the third-party licences, which runs `app.licenses`): it has no timer, and goes on a click outside the links or on Escape, as the splash at the start does",
    },
    UiCommand {
        id: "app.licenses",
        label: "Third-Party Licences",
        menu: &["File"],
        shortcut: None,
        params: "close: true to close the window at once, and without it the window opens (it shows THIRD-PARTY-LICENSES.txt, the libraries DiaCraft is built from and the licences they are under, in a \
                 list that scrolls with the wheel, the bar, Page Up, Page Down, Home, End and the arrow keys, needs no drawing and goes on Escape or its Close button)",
    },
    UiCommand { id: "app.ownWindow", label: "Open in Its Own Window", menu: &["File"], shortcut: None, params: "none" },
    UiCommand {
        id: "window.fullScreen",
        label: "Full Screen",
        menu: &["View", "Views"],
        shortcut: None,
        params: "on: true or false, optional: true goes to full screen with the keyboard locked (a click is what lets the browser do it), false leaves it, and without it the window turns the other way from how it is",
    },
    UiCommand { id: "app.closeWindow", label: "Close Window", menu: &["File"], shortcut: None, params: "none" },
];

/// The window command with this id.
pub fn find_ui_command(id: &str) -> Option<&'static UiCommand> {
    UI_COMMANDS.iter().find(|c| c.id == id)
}

/// Whether `id` names a command in either registry.
pub fn known(id: &str) -> bool {
    find_command(id).is_some() || find_ui_command(id).is_some()
}

/// Whether running the command `id` first ends a text edit that is open and sends what was typed (of the M3 review): every engine
/// command that changes something (a query does not: a program that polls `document.inspect` must not close a person's text box), and the
/// window's commands that change the page or the shapes selected, or leave the drawing area for the File view. Zoom, panels, the theme and
/// the tool leave the box open.
pub(crate) fn ends_text_edit(id: &str) -> bool {
    match find_command(id) {
        Some(spec) => spec.journal,
        // `format.*` send what was typed themselves, because they need the characters selected in the box before it closes (`format.rs`).
        None => matches!(
            id,
            "page.select"
                | "edit.select"
                | "edit.selectAll"
                | "edit.selectByType"
                | "edit.selectRegion"
                | "window.selectByType"
                | "window.insertPicture"
                | "window.callout"
                | "window.relayout"
                | "window.autoSpace"
                | "arrange.moveOffPageBreaks"
                | "arrange.avoidPageBreaks"
                | "window.followLink"
                | "window.backstage"
                | "window.pageSetup"
                | "window.reorderPages"
                | "window.snapGlue"
                | "window.layerProperties"
                | "window.assignLayer"
                | "window.defineShapeData"
                | "window.askShapeData"
                | "window.containerAdd"
                | "window.containerDo"
                | "window.swimlaneDo"
                | "window.orgChartDo"
                | "window.zoomDialog"
                | "window.find"
                | "format.rotateText"
                | "format.painter"
                | "window.tab"
                | "arrange.rotate"
                | "arrange.flip"
                | "arrange.order"
                | "arrange.align"
                | "arrange.changeShape"
                | "arrange.group"
                | "arrange.ungroup"
                | "clipboard.copy"
                | "clipboard.cut"
                | "clipboard.paste"
                | "clipboard.duplicate"
                | "operations.union"
                | "operations.combine"
                | "operations.fragment"
                | "operations.intersect"
                | "operations.subtract"
        ),
    }
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

fn number(cmd: &str, p: &Map<String, Value>, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

fn flag(cmd: &str, p: &Map<String, Value>, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn text<'a>(cmd: &str, p: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    match p.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
        None => Err(bad(cmd, format!("missing `{key}`"))),
    }
}

fn optional_text<'a>(cmd: &str, p: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

/// The rectangle the zoom and fit commands work in before the window has drawn a frame to say how big the drawing area is.
fn fallback_area() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))
}

impl DiacraftApp {
    /// Runs the window command `id` if there is one. `None`: it is not a window command, so the engine has it.
    pub(crate) fn run_window_command(&mut self, id: &str, params: &Value) -> Option<Result<Value>> {
        let spec = find_ui_command(id)?;
        Some(self.run_ui_spec(spec, params))
    }

    fn run_ui_spec(&mut self, spec: &UiCommand, params: &Value) -> Result<Value> {
        let map = match params {
            Value::Null => Map::new(),
            Value::Object(m) => m.clone(),
            _ => return Err(bad(spec.id, "parameters must be a JSON object")),
        };
        let keys = param_keys(spec.params).map_err(|e| EngineError::Internal { cmd: spec.id.to_owned(), msg: e })?;
        if let Some(unknown) = map.keys().find(|k| !keys.contains(&k.as_str())) {
            let known =
                if keys.is_empty() { "the command takes no parameters".to_owned() } else { format!("the command takes: {}", keys.join(", ")) };
            return Err(bad(spec.id, format!("unknown parameter `{}`; {known}", shown(unknown, 64))));
        }
        // A dialog can always be closed.
        let closing = matches!(map.get("close"), Some(Value::Bool(true)))
            && matches!(
                spec.id,
                "window.templateDetail"
                    | "window.textDialog"
                    | "window.pageSetup"
                    | "window.reorderPages"
                    | "window.snapGlue"
                    | "window.layerProperties"
                    | "window.assignLayer"
                    | "window.defineShapeData"
                    | "window.askShapeData"
                    | "window.options"
                    | "window.zoomDialog"
                    | "window.colorsDialog"
                    | "window.relayoutOptions"
                    | "window.spacingOptions"
                    | "window.find"
                    | "window.hyperlinks"
                    | "window.field"
                    | "window.symbols"
                    | "app.about"
                    | "app.licenses"
            );
        if !closing {
            self.window_command_enabled(spec.id).map_err(|reason| EngineError::Disabled { cmd: spec.id.to_owned(), reason })?;
        }
        self.apply_window_command(spec.id, &map)
    }

    /// Whether the window command `id` can run now, and if not why.
    pub(crate) fn window_command_enabled(&self, id: &str) -> std::result::Result<(), String> {
        // The shape operations ask the engine, once for each drawing, revision, page and selection (`operations.rs`).
        if let Some(op) = crate::operations::op_of_command(id) {
            return self.shape_op_verdict(op);
        }
        match id {
            "view.zoomIn"
            | "view.zoomOut"
            | "view.zoom"
            | "view.fitPage"
            | "view.fitWidth"
            | "window.zoomDialog"
            | "window.find"
            | "view.pan"
            | "page.select"
            | "window.pane"
            | "window.paneDock"
            | "window.paneTab"
            | "window.paneMove"
            | "window.explorerBranch"
            | "window.newComment"
            | "window.showComment"
            | "window.stepComment"
            | "tool.pointer"
            | "tool.connector"
            | "tool.text"
            | "tool.text_block"
            | "tool.rectangle"
            | "tool.ellipse"
            | "tool.line"
            | "tool.freeform"
            | "tool.arc"
            | "tool.pencil"
            | "tool.area_select"
            | "tool.lasso_select"
            | "window.selectByType"
            | "edit.selectByType"
            | "edit.selectRegion"
            | "window.selectPartially"
            | "window.freeformSmoothing"
            | "window.pageSetup"
            | "edit.select"
            | "edit.selectAll"
            | "print.set"
            | "print.start" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else {
                    Ok(())
                }
            }
            // They act on the selection.
            "draw.closeShape"
            | "format.shape"
            | "format.gradient"
            | "format.shadow"
            | "format.options"
            | "format.text"
            | "format.toggle"
            | "format.size"
            | "format.rotateText"
            | "window.textDialog"
            | "window.colorsDialog"
            | "arrange.rotate"
            | "arrange.flip"
            | "arrange.order" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().is_empty() {
                    Err("nothing is selected".to_owned())
                } else {
                    Ok(())
                }
            }
            // Putting pages in order needs two pages, as `page.move` does, and the dialog puts the pages that are not backgrounds in order (a background has no place among
            // them), so it needs two of those.
            "window.reorderPages" => {
                self.command_enabled("page.move")?;
                let pages = self.session().document().map_or(0, |doc| doc.pages.iter().filter(|p| !p.background).count());
                if pages < 2 { Err("the drawing has one page that is not a background".to_owned()) } else { Ok(()) }
            }
            // The dialog sends `snap.set`, which needs a document.
            "window.snapGlue" => self.command_enabled("snap.set"),
            // The Insert tab: a picture needs a document, Link the one shape selected, Field and Symbol a text box that is open or the one shape selected.
            "window.insertPicture" => self.command_enabled("insert.picture"),
            "window.callout" => self.callout_enabled(),
            // Re-Layout Page, Auto Space and the Position menu's items work on a drawing with shapes ("Auto Space ... work on the selected
            // shapes, or on every shape when nothing is selected"), so the menu is quiet in an empty drawing.
            "window.relayout"
            | "window.relayoutOptions"
            | "window.autoSpace"
            | "arrange.moveOffPageBreaks"
            | "window.spacingOptions"
            | "arrange.avoidPageBreaks" => self.relayout_enabled(),
            "view.pageBreaks" => self.command_enabled("page_breaks.set"),
            // The detail of a template makes the drawing with `file.new`.
            "window.templateDetail" => self.command_enabled("file.new"),
            "window.hyperlinks" => self.links_enabled(),
            "window.followLink" => self.command_enabled("hyperlink.follow"),
            "window.field" => self.field_enabled(),
            "window.symbols" | "window.symbol" => self.symbol_enabled(),
            // The layers' dialogs send `layer.*`, which need a document; Assign to Layer works on the shapes selected.
            "window.layerProperties" => self.command_enabled("layer.add"),
            // The Shape Data dialog sends `shapedata.define` for the one shape selected.
            "window.defineShapeData" => self.define_data_enabled(),
            "window.askShapeData" => self.command_enabled("shapedata.set"),
            "window.assignLayer" => {
                self.command_enabled("layer.assign")?;
                if self.ui.edit.selection().is_empty() { Err("nothing is selected".to_owned()) } else { Ok(()) }
            }
            // The containers: one is added anywhere there is a drawing; the rest act on the shapes selected (`container_ui.rs`).
            "window.containerAdd" => self.command_enabled("container.add"),
            // The swimlanes: the page on view has a structure of lanes (`swimlane_ui.rs`).
            "window.swimlaneDo" => {
                self.command_enabled("swimlane.info")?;
                if self.swimlane_present() { Ok(()) } else { Err("the page has no swimlanes".to_owned()) }
            }
            "window.containerDo" => {
                self.command_enabled("container.info")?;
                if self.ui.edit.selection().is_empty() { Err("nothing is selected".to_owned()) } else { Ok(()) }
            }
            // The Org Chart tab (`org_ui.rs`): where there is a chart.
            "window.orgChartDo" => self.command_enabled("orgchart.relayout"),
            // The box is the drawing's and the program's setting is over it: greyed out while that is off.
            // The browser version's own commands (`page_host.rs`): they need the page of a browser.
            "app.ownWindow" => self.own_window_enabled(),
            "window.fullScreen" => self.full_screen_enabled(),
            "app.closeWindow" => self.close_window_enabled(),
            "view.autoConnect" => {
                if !self.ui.enable_autoconnect {
                    Err("AutoConnect is turned off for the program (File, Options, Advanced, Enable AutoConnect)".to_owned())
                } else {
                    self.command_enabled("autoconnect.set")
                }
            }
            "arrange.align" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().len() < 2 {
                    Err("select two shapes or more: the others move to the first".to_owned())
                } else {
                    Ok(())
                }
            }
            // Change Shape works on the shapes selected, and the document may turn swapping off (`DocLockReplace`).
            "arrange.changeShape" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().is_empty() {
                    Err("nothing is selected".to_owned())
                } else {
                    self.command_enabled("shape.change")
                }
            }
            "clipboard.duplicate" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().is_empty() {
                    Err("nothing is selected".to_owned())
                } else {
                    Ok(())
                }
            }
            // Copy, Cut and Paste of the ribbon and the keys leave the text of an open box to the box.
            "clipboard.copy" | "clipboard.cut" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.editing_text() {
                    Err("a text box is open: its text is copied and cut by its own keys".to_owned())
                } else if self.ui.edit.selection().is_empty() {
                    Err("nothing is selected".to_owned())
                } else {
                    Ok(())
                }
            }
            "clipboard.paste" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.editing_text() {
                    Err("a text box is open: its text is pasted by its own key".to_owned())
                } else if self.session().clipboard().is_none_or(|clip| clip.is_empty()) {
                    Err("the clipboard is empty".to_owned())
                } else {
                    Ok(())
                }
            }
            "format.painter" => self.format_painter_enabled(),
            "format.quickStyle" => match self.quick_refusal() {
                Some(why) => Err(why),
                None if self.ui.edit.selection().is_empty() => Err("nothing is selected".to_owned()),
                None => Ok(()),
            },
            "arrange.group" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().len() < 2 {
                    Err("select two shapes or more to group".to_owned())
                } else if !self.selection_in_one_list() {
                    Err("the shapes are in different groups: a group is made of shapes in one list".to_owned())
                } else {
                    Ok(())
                }
            }
            "arrange.ungroup" => {
                if self.session().document().is_none() {
                    Err("no document open".to_owned())
                } else if self.ui.edit.selection().is_empty() {
                    Err("nothing is selected".to_owned())
                } else if !self.selection_has_group() {
                    Err("no shape selected is a group".to_owned())
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Whether the command `id`, the window's or the engine's, can run now, and if not why.
    pub fn command_enabled(&self, id: &str) -> std::result::Result<(), String> {
        if find_ui_command(id).is_some() {
            return self.window_command_enabled(id);
        }
        // A variant is a variant of the page's theme, which only the window knows the page of (`ribbon/design.rs`).
        if id == "theme.variant"
            && let Some(why) = self.variant_refusal()
        {
            return Err(why);
        }
        match find_command(id) {
            // Delete acts on the selection, which only the window knows.
            Some(_) if id == "shape.delete" && self.ui.edit.selection().is_empty() && self.ui.edit.guide().is_none() => {
                Err("nothing is selected".to_owned())
            }
            // Export from the window needs the app's Save dialog to name the file, as Open does, and one at a time.
            Some(_) if id == "file.export" && !self.export_dialog_available() => {
                Err("there is no file dialog to name the picture with now".to_owned())
            }
            Some(spec) => (spec.enabled)(self.session()),
            None => Err(format!("unknown command `{}`", shown(id, 64))),
        }
    }

    fn apply_window_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<Value> {
        match id {
            "view.zoomIn" => self.zoom_step(1.25),
            "view.zoomOut" => self.zoom_step(0.8),
            "view.zoom" => {
                let percent = number(id, p, "percent")?.ok_or_else(|| bad(id, "missing `percent`"))?;
                let (x, y) = (number(id, p, "x")?, number(id, p, "y")?);
                let anchor = match (x, y) {
                    (Some(x), Some(y)) => Some(egui::pos2(x as f32, y as f32)),
                    (None, None) => None,
                    _ => return Err(bad(id, "`x` and `y` go together")),
                };
                self.zoom_to(percent / 100.0, anchor);
            }
            "view.fitPage" => self.fit(false),
            "view.fitWidth" => self.fit(true),
            "window.zoomDialog" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_zoom_dialog();
                } else {
                    self.open_zoom_dialog();
                }
            }
            "window.colorsDialog" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_colors_dialog();
                } else {
                    let target = match p.get("target") {
                        Some(Value::String(word)) => word.as_str(),
                        _ => return Err(bad(id, "`target` must be fill, line, color, stop_color, line_stop_color, fill_background or shadow_color")),
                    };
                    self.open_colors_dialog(target)?;
                }
            }
            "window.find" => self.find_command(p)?,
            "view.pan" => {
                let (dx, dy) = (number(id, p, "dx")?.unwrap_or(0.0), number(id, p, "dy")?.unwrap_or(0.0));
                self.with_view(|v, _| v.pan_by(egui::vec2(dx as f32, dy as f32)));
            }
            // The View settings are the drawing's: the box turned is shown, and written into the drawing by `window.settings.set` (`window_settings.rs`).
            "view.grid" => {
                let on = flag(id, p, "on")?.unwrap_or(!self.ui.show_grid);
                self.turn_view_settings(WindowFlags { grid: Some(on), ..WindowFlags::default() });
            }
            "view.rulers" => {
                let on = flag(id, p, "on")?.unwrap_or(!self.ui.show_rulers);
                self.turn_view_settings(WindowFlags { rulers: Some(on), ..WindowFlags::default() });
            }
            "view.guides" => {
                let on = flag(id, p, "on")?.unwrap_or(!self.ui.show_guides);
                self.turn_view_settings(WindowFlags { guides: Some(on), ..WindowFlags::default() });
            }
            "view.connectionPoints" => {
                let on = flag(id, p, "on")?.unwrap_or(!self.ui.show_connection_points);
                self.turn_view_settings(WindowFlags { connection_points: Some(on), ..WindowFlags::default() });
            }
            // The drawing's own box (`autoconnect.set`: a change a save keeps and no undo step) and the program-wide switch over it.
            "view.autoConnect" => {
                let on = flag(id, p, "on")?.unwrap_or(!self.autoconnect_box());
                self.execute("autoconnect.set", &json!({ "on": on }))?;
            }
            "window.enableAutoConnect" => {
                self.ui.enable_autoconnect = flag(id, p, "on")?.unwrap_or(!self.ui.enable_autoconnect);
                self.autoconnect_settings_changed();
            }
            "window.snapGlue" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_snap_dialog();
                } else {
                    let name = text(id, p, "tab")?;
                    let tab = crate::snap_dialog::SnapTab::from_name(name)
                        .ok_or_else(|| bad(id, format!("no tab `{}`; the tabs are: General, Advanced", shown(name, 32))))?;
                    self.open_snap_dialog(tab)?;
                }
            }
            "window.layerProperties" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_layer_properties();
                } else {
                    self.open_layer_properties()?;
                }
            }
            "window.defineShapeData" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_define_shape_data();
                } else {
                    self.open_define_shape_data()?;
                }
            }
            "window.askShapeData" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_ask_data();
                } else {
                    let shape = number(id, p, "id")?
                        .filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX))
                        .ok_or_else(|| bad(id, "`id` must be a whole number, the shape"))?;
                    let page = match number(id, p, "page")? {
                        Some(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => n as u32,
                        Some(_) => return Err(bad(id, "`page` must be a whole number, the id of a page")),
                        None => self.current_page().ok_or_else(|| bad(id, "no document open"))?,
                    };
                    self.ask_for_data(page, shape as u32);
                }
            }
            "window.assignLayer" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_assign_layer();
                } else {
                    self.open_assign_layer()?;
                }
            }
            "window.containerAdd" | "window.containerDo" => self.container_window_command(id, p)?,
            "window.swimlaneDo" => self.swimlane_window_command(id, p)?,
            "window.orgChartDo" => self.org_window_command(p)?,
            "window.ribbon" => self.ui.ribbon_collapsed = flag(id, p, "collapsed")?.unwrap_or(!self.ui.ribbon_collapsed),
            "window.shapes" => self.ui.shapes_collapsed = !flag(id, p, "shown")?.unwrap_or(self.ui.shapes_collapsed),
            "window.stencil" => {
                let which = text(id, p, "stencil")?;
                if !self.show_stencil(which) {
                    let names: Vec<String> = self.stencil_list().iter().map(|s| s.name.clone()).collect();
                    return Err(bad(id, format!("no stencil `{}`; the stencils are: {}", shown(which, 32), names.join(", "))));
                }
            }
            "window.shapeFilter" => self.set_shape_filter(text(id, p, "query")?),
            "shapes.search" => {
                let text = match p.get("text") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(text)) => Some(text.as_str()),
                    Some(_) => return Err(bad(id, "`text` must be text")),
                };
                let mode = match p.get("mode") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(mode)) => Some(mode.as_str()),
                    Some(_) => return Err(bad(id, "`mode` must be text")),
                };
                self.command_search(text, mode)?;
            }
            "shapes.openStencil" => {
                let path = match p.get("path") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(path)) => Some(path.as_str()),
                    Some(_) => return Err(bad(id, "`path` must be text")),
                };
                self.command_open_stencil(path)?;
            }
            "shapes.closeStencil" => self.close_stencil(text(id, p, "id")?)?,
            "window.tab" => self.open_tab(text(id, p, "tab")?)?,
            "window.theme" => {
                self.ui.theme = match text(id, p, "theme")? {
                    "light" => egui::Theme::Light,
                    "dark" => egui::Theme::Dark,
                    other => return Err(bad(id, format!("`theme` must be light or dark, not `{}`", shown(other, 32)))),
                };
            }
            "window.backstage" => {
                let page = backstage_page(id, text(id, p, "page")?)?;
                if page == Some(BackstagePage::Print) && !self.can_open_print() {
                    return Err(EngineError::Disabled { cmd: id.to_owned(), reason: "no document open".to_owned() });
                }
                self.ui.new_page.detail = None;
                self.ui.backstage = page;
            }
            "window.templateDetail" => self.template_detail_command(p)?,
            "print.set" => self.print_set(p)?,
            "print.start" => self.print_start(p)?,
            "window.developer" => {
                self.ui.developer_tab = flag(id, p, "on")?.unwrap_or(!self.ui.developer_tab);
                if !self.ui.developer_tab && self.ui.tab == "Developer" {
                    self.ui.tab = crate::state::FIRST_TAB.to_owned();
                }
            }
            "page.select" => self.select_page(number(id, p, "id")?.ok_or_else(|| bad(id, "missing `id`"))?)?,
            "tool.pointer" => self.set_tool(crate::edit::Tool::Pointer),
            "tool.connector" => self.set_tool(crate::edit::Tool::Connector),
            "window.textDialog" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_text_dialog();
                } else {
                    let name = text(id, p, "tab")?;
                    let tab = crate::text_dialog::TextTab::from_name(name)
                        .ok_or_else(|| bad(id, format!("no tab `{}`; the tabs are: Font, Paragraph, Text Block", shown(name, 32))))?;
                    self.open_text_dialog(tab)?;
                }
            }
            "app.about" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_splash();
                } else {
                    self.show_about();
                }
            }
            "app.ownWindow" => self.run_own_window(),
            "window.fullScreen" => self.run_full_screen(flag(id, p, "on")?),
            "app.closeWindow" => self.ask_to_close(),
            "app.licenses" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_licenses();
                } else {
                    // The About panel hands over to the window: both are modals, and the window is the one that is wanted.
                    self.close_splash();
                    self.open_licenses();
                }
            }
            "window.pane" => self.pane_command(text(id, p, "pane")?, flag(id, p, "shown")?)?,
            "window.paneDock" => {
                let with = match p.get("with") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(w)) => Some(w.as_str()),
                    Some(_) => return Err(bad(id, "`with` must be text")),
                };
                self.pane_dock_command(text(id, p, "pane")?, with)?;
            }
            "window.paneTab" => self.pane_tab_command(text(id, p, "pane")?)?,
            "window.paneMove" => self.pane_move_command(text(id, p, "pane")?, flag(id, p, "up")?.ok_or_else(|| bad(id, "missing `up`"))?)?,
            "window.explorerBranch" => self.explorer_branch_command(text(id, p, "branch")?, flag(id, p, "open")?)?,
            "window.newComment" => {
                let whole = |key: &str| -> Result<Option<u32>> {
                    match p.get(key) {
                        None | Some(Value::Null) => Ok(None),
                        Some(v) => {
                            v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| bad(id, format!("`{key}` must be a whole number")))
                        }
                    }
                };
                self.new_comment_command(whole("id")?, whole("page")?)?;
            }
            "window.showComment" => {
                let comment = p
                    .get("comment")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| bad(id, "`comment` must be a whole number"))?;
                self.show_comment_command(comment)?;
            }
            "window.stepComment" => self.step_comment_command(flag(id, p, "forward")?.unwrap_or(true))?,
            "tool.area_select" => self.set_tool(crate::edit::Tool::AreaSelect),
            "tool.lasso_select" => self.set_tool(crate::edit::Tool::Lasso),
            "window.selectByType" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_select_by_type();
                } else {
                    self.open_select_by_type()?;
                }
            }
            "window.insertPicture" => {
                let path = match p.get("path") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(path)) => Some(path.as_str()),
                    Some(_) => return Err(bad(id, "`path` must be text")),
                };
                self.command_insert_picture(path)?;
            }
            "window.relayout" => self.window_relayout(p)?,
            "window.relayoutOptions" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_relayout_dialog();
                } else {
                    self.open_relayout_dialog()?;
                }
            }
            "window.autoSpace" => self.window_autospace()?,
            "window.spacingOptions" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_spacing_dialog();
                } else {
                    self.open_spacing_dialog()?;
                }
            }
            "arrange.moveOffPageBreaks" => self.window_move_off_breaks()?,
            "arrange.avoidPageBreaks" => self.window_avoid_breaks(flag(id, p, "on")?)?,
            "view.pageBreaks" => self.window_page_breaks(flag(id, p, "on")?)?,
            "window.callout" => {
                let style = match p.get("style") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(style)) => Some(style.as_str()),
                    Some(_) => return Err(bad(id, "`style` must be text")),
                };
                self.insert_callout(style)?;
            }
            "window.hyperlinks" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_links();
                } else {
                    self.open_links()?;
                }
            }
            "window.followLink" => {
                let shape = number(id, p, "id")?
                    .filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX))
                    .ok_or_else(|| bad(id, "`id` must be a whole number, the shape"))?;
                self.command_follow_link(shape as u32)?;
            }
            "window.field" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_field();
                } else {
                    self.open_field()?;
                }
            }
            "window.symbols" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_symbols();
                } else {
                    self.open_symbols()?;
                }
            }
            "window.symbol" => {
                self.command_symbol(text(id, p, "text")?)?;
            }
            "edit.selectByType" => self.select_by_type_command(p)?,
            "edit.selectRegion" => self.select_region_command(p)?,
            "window.selectPartially" => self.ui.select_partial = flag(id, p, "on")?.unwrap_or(!self.ui.select_partial),
            "tool.text" => self.set_tool(crate::edit::Tool::Text),
            "tool.text_block" => self.set_tool(crate::edit::Tool::TextBlock),
            "tool.rectangle" | "tool.ellipse" | "tool.line" | "tool.freeform" | "tool.arc" | "tool.pencil" => {
                if let Some(pen) = crate::edit::tool_of_command(id) {
                    self.set_tool(crate::edit::Tool::Draw(pen));
                }
            }
            "draw.closeShape" => self.close_selected_shapes()?,
            "window.freeformSmoothing" => {
                if let Some(percent) = number(id, p, "percent")? {
                    if !(0.0..=100.0).contains(&percent) {
                        return Err(bad(id, "`percent` must be a number from 0 to 100"));
                    }
                    self.ui.freeform_smoothing = percent / 100.0;
                }
            }
            "format.rotateText" => self.rotate_text(number(id, p, "by")?.ok_or_else(|| bad(id, "missing `by`"))?)?,
            "window.pageSetup" => self.page_setup_command(p)?,
            "window.reorderPages" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.ui.page_setup.reorder = None;
                } else {
                    self.open_reorder_pages();
                }
            }
            "edit.select" => self.select_by_command(p)?,
            "edit.selectAll" => self.select_all(),
            "format.shape" => self.format_shape(p)?,
            "format.gradient" => self.format_gradient(p)?,
            "format.shadow" => self.format_shadow(p)?,
            "format.quickStyle" => self.format_quick_style(p)?,
            "format.options" => {
                let tab = match p.get("tab") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(tab)) => Some(tab.as_str()),
                    Some(_) => return Err(bad(id, "`tab` must be text")),
                };
                self.format_options_command(tab)?;
            }
            "format.painter" => {
                let to = match p.get("to") {
                    None | Some(Value::Null) => None,
                    Some(Value::Array(list)) => Some(
                        list.iter()
                            .take(65_536)
                            .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(id, "`to` must hold whole numbers")))
                            .collect::<Result<Vec<u32>>>()?,
                    ),
                    Some(_) => return Err(bad(id, "`to` must be an array of whole numbers")),
                };
                self.format_painter_command(flag(id, p, "on")?, flag(id, p, "keep")?.unwrap_or(false), to)?;
            }
            "format.text" => self.format_text(p)?,
            "format.toggle" => self.format_toggle(text(id, p, "what")?)?,
            "format.size" => self.format_size_step(text(id, p, "step")?)?,
            "arrange.rotate" => self.arrange_rotate(number(id, p, "by")?.ok_or_else(|| bad(id, "missing `by`"))?)?,
            "arrange.flip" => self.arrange_flip(text(id, p, "axis")?)?,
            "arrange.order" => self.arrange_order(text(id, p, "to")?)?,
            "arrange.align" => self.arrange_align(text(id, p, "how")?)?,
            "arrange.changeShape" => self.arrange_change_shape(text(id, p, "master")?, optional_text(id, p, "stencil")?)?,
            "arrange.group" => self.arrange_group()?,
            "arrange.ungroup" => self.arrange_ungroup()?,
            "operations.union" => self.run_shape_op(Op::Union)?,
            "operations.combine" => self.run_shape_op(Op::Combine)?,
            "operations.fragment" => self.run_shape_op(Op::Fragment)?,
            "operations.intersect" => self.run_shape_op(Op::Intersect)?,
            "operations.subtract" => self.run_shape_op(Op::Subtract)?,
            "window.options" => {
                if flag(id, p, "close")?.unwrap_or(false) {
                    self.close_options_dialog();
                } else {
                    let name = text(id, p, "tab")?;
                    let tab = crate::options_dialog::OptionsTab::from_name(name)
                        .ok_or_else(|| bad(id, format!("no tab `{}`; the tabs that work are: Advanced, Customize Ribbon", shown(name, 32))))?;
                    self.open_options_dialog(tab);
                }
            }
            "clipboard.copy" => self.clipboard_copy()?,
            "clipboard.cut" => self.clipboard_cut()?,
            "clipboard.paste" => self.clipboard_paste(number(id, p, "x")?, number(id, p, "y")?)?,
            "clipboard.duplicate" => self.clipboard_duplicate(number(id, p, "dx")?, number(id, p, "dy")?)?,
            other => return Err(EngineError::UnknownCommand(shown(other, 64))),
        }
        Ok(self.window_state())
    }

    /// The window state a window command answers with.
    pub fn window_state(&self) -> Value {
        let view = self.ui.page.and_then(|p| self.ui.views.get(&p));
        let mut state = json!({
            "tab": self.ui.tab,
            "ribbonCollapsed": self.ui.ribbon_collapsed,
            "shapesCollapsed": self.ui.shapes_collapsed,
            "stencil": self.ui.stencils.list.as_ref().and_then(|list| {
                let chosen = self.ui.stencils.current.as_ref().and_then(|id| list.iter().find(|s| &s.id == id));
                chosen.or(list.first()).map(|s| s.id.clone())
            }),
            "shapeFilter": self.ui.stencils.filter,
            "shapeFilterCount": self.shape_filter_count(),
            "grid": self.ui.show_grid,
            "rulers": self.ui.show_rulers,
            "guides": self.ui.show_guides,
            "connectionPoints": self.ui.show_connection_points,
            "guide": self.ui.edit.guide(),
            "page": self.ui.page,
            "zoomPercent": view.map(|v| (v.zoom * 1000.0).round() / 10.0),
            "backstage": self.ui.backstage.map(BackstagePage::name),
            "theme": if self.ui.theme == egui::Theme::Dark { "dark" } else { "light" },
            // Where the page is on the screen, in points (left, top, width, height), and points to a paper inch: what a client needs to aim
            // a drag at a place of the page.
            "pageRect": self.ui.report.page.map(|r| [r.min.x, r.min.y, r.width(), r.height()]),
            "pointsPerInch": view.map(crate::canvas::view::View::scale),
            "tool": self.ui.edit.tool.name(),
            "formatPainter": self.format_painter_state(),
            "textBlock": self.text_block_shown().map(|b| json!({"corners": b.corners, "pin": b.pin, "direction": b.direction})),
            "panes": self.panes_state(),
            "explorerOpen": self.explorer_state(),
            "textDialog": self.text_dialog_tab(),
            "pageSetup": self.page_setup_tab(),
            "reorderPages": self.reorder_pages_open(),
            "snapGlue": self.snap_dialog_tab(),
            "zoomDialog": self.zoom_dialog_choice(),

            "options": self.options_dialog_tab(),
            "selection": self.ui.edit.selection(),
            "editingText": self.ui.edit.editing_text(),
            "dirty": self.is_dirty(),
            "print": self.print_state_json(),
            // Where the drag in progress would land, and the `snap` the last drag's command answered with.
            "drag": self.drag_preview().map(|d| json!({
                "delta": d.delta,
                "sized": d.sized,
                "turned": d.turned,
                "corners": d.corners,
                "guideAt": d.guide_at,
                "snap": d.snap,
                "marks": d.marks.iter().map(crate::edit::snapping::mark_json).collect::<Vec<_>>(),
            })),
            "lastSnap": self.last_snap(),
        });
        // Not in the `json!` above: it is at the macro's recursion limit. The splash or About panel while it is up (`start` or `about`), else null.
        if let Some(map) = state.as_object_mut() {
            map.insert("splash".to_owned(), json!(self.splash_kind().map(crate::splash::SplashKind::name)));
            // The Third-party licences window while it is open: its rows and where it is scrolled to; else null.
            map.insert("licenses".to_owned(), self.licenses_state());
            map.insert("autoConnect".to_owned(), self.autoconnect_state());
            // The search for shapes (`null` when none is shown): the words, the mode, how many masters matched, and whether it reached the end.
            map.insert("shapeSearch".to_owned(), self.shape_search_state());
            // The option "Select shapes partially within area", and the Select by Type dialog (`null` when it is closed, else what is ticked).
            map.insert("selectPartially".to_owned(), json!(self.ui.select_partial));
            map.insert("selectByType".to_owned(), self.select_by_type_state());
            // The Colors dialog (`null` when closed), the Recent Colors, and the theme or variant shown on the page under the pointer.
            map.insert("colorsDialog".to_owned(), self.colors_dialog_state());
            map.insert("recentColours".to_owned(), json!(self.recent_colours().iter().map(|c| c.hex()).collect::<Vec<_>>()));
            map.insert("themePreview".to_owned(), self.theme_preview_state());
            map.insert("find".to_owned(), self.find_state());
            // The Insert tab: the dialogs of Link, Field and Symbol (`null` when closed), whether the picture chooser is asked for or on the screen, and the last address handed to the system.
            map.insert("hyperlinks".to_owned(), self.links_state());
            map.insert("field".to_owned(), self.field_state());
            map.insert("symbols".to_owned(), self.symbol_state());
            map.insert("pictureDialog".to_owned(), json!(self.picture_dialog_open()));
            map.insert("openedUrl".to_owned(), json!(self.ui.insert.last_url));
            // The drag in progress with a drawing tool, and the Freeform tool's smoothing in percent (`edit/draw.rs`).
            map.insert("drawing".to_owned(), self.drawing_state());
            map.insert("freeformSmoothing".to_owned(), json!((self.freeform_smoothing() * 100.0).round()));
            // The handles on the legs of the connector selected, its reroute setting and the drag of a leg (`edit/legs.rs`).
            map.insert("legs".to_owned(), self.legs_state());
            // Whether the Layer Properties and Assign to Layer dialogs are open (`layers_dialog.rs`).
            map.insert("layerProperties".to_owned(), json!(self.layer_properties_open()));
            // The Define Shape Data dialog and the question a drop asks (`null` when closed; `shape_data.rs`).
            map.insert("shapeData".to_owned(), self.shape_data_state());
            map.insert("assignLayer".to_owned(), json!(self.assign_layer_open()));
            // The containers among the shapes selected, and the containers that hold a member selected (`container_ui.rs`).
            map.insert("container".to_owned(), self.container_state());
            // Whether the page has swimlanes, how they lie and which parts are selected (`swimlane_ui.rs`).
            map.insert("swimlane".to_owned(), self.swimlane_state());
            map.insert("orgChart".to_owned(), self.org_state());
            // Re-Layout Page: the style the Configure Layout dialog has (`null` when closed), whether Spacing Options is open, and the Page Breaks box.
            map.insert("relayoutDialog".to_owned(), json!(self.relayout_dialog_style()));
            map.insert("spacingDialog".to_owned(), json!(self.spacing_dialog_open()));
            map.insert("pageBreaks".to_owned(), json!(self.page_breaks_box()));
        }
        state
    }

    /// `window.pageSetup`: opens the Page Setup dialog on a tab, or closes it.
    fn page_setup_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        const ID: &str = "window.pageSetup";
        if flag(ID, p, "close")?.unwrap_or(false) {
            self.close_page_setup();
            return Ok(());
        }
        let name = text(ID, p, "tab")?;
        let tab = crate::page_setup::DialogTab::from_name(name).ok_or_else(|| {
            let names: Vec<&str> = crate::page_setup::DialogTab::ALL.iter().filter(|t| t.works()).map(|t| t.name()).collect();
            bad(ID, format!("no tab `{}`; the tabs are: {}", shown(name, 32), names.join(", ")))
        })?;
        let page = match number(ID, p, "page")? {
            None => None,
            Some(n) => Some(
                u32::try_from(n as i64)
                    .ok()
                    .filter(|id| n >= 0.0 && n.fract() == 0.0 && self.session().document().is_some_and(|d| d.page(*id).is_some()))
                    .ok_or_else(|| bad(ID, "no page has that id"))?,
            ),
        };
        self.open_page_setup(tab, page)
    }

    /// `edit.selectRegion`: selects what `select.by_region` takes for the page on view, with the window's option unless `partial` is given, after what was selected when `add` is true.
    fn select_region_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        const ID: &str = "edit.selectRegion";
        let page = self.current_page().ok_or_else(|| bad(ID, "no document open"))?;
        let add = flag(ID, p, "add")?.unwrap_or(false);
        let mut params = Map::new();
        for key in ["polygon", "rect", "partial"] {
            if let Some(value) = p.get(key) {
                params.insert(key.to_owned(), value.clone());
            }
        }
        let base = if add && self.ui.edit.selection_page() == Some(page) { self.ui.edit.selection().to_vec() } else { Vec::new() };
        self.select_in_region(page, Value::Object(params), base)
    }

    fn select_by_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        const ID: &str = "edit.select";
        let Some(Value::Array(list)) = p.get("ids") else { return Err(bad(ID, "`ids` must be an array of whole numbers")) };
        let mut ids = Vec::with_capacity(list.len().min(65_536));
        for value in list.iter().take(65_536) {
            let id = value.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(ID, "`ids` must hold whole numbers"))?;
            ids.push(id);
        }
        self.select(&ids);
        Ok(())
    }

    fn open_tab(&mut self, label: &str) -> Result<()> {
        let names = crate::ribbon::spec::tab_names(self.tab_context());
        match names.iter().find(|n| n.eq_ignore_ascii_case(label)) {
            Some(&crate::ribbon::spec::FILE_TAB) => self.ui.backstage = Some(BackstagePage::Home),
            Some(name) => {
                self.ui.tab = (*name).to_owned();
                self.ui.ribbon_collapsed = false;
            }
            None => return Err(bad("window.tab", format!("no tab `{}`; the tabs are: {}", shown(label, 32), names.join(", ")))),
        }
        Ok(())
    }

    /// The tabs that show now.
    pub fn tab_context(&self) -> crate::ribbon::spec::TabContext {
        crate::ribbon::spec::TabContext {
            developer: self.ui.developer_tab,
            org_chart: self.org_chart_present(),
            container: self.container_selected(),
            swimlane: self.swimlane_present(),
        }
    }

    fn select_page(&mut self, id: f64) -> Result<()> {
        let wanted = u32::try_from(id as i64).ok().filter(|_| id >= 0.0 && id.fract() == 0.0);
        let Some(doc) = self.session().document() else { return Err(bad("page.select", "no document open")) };
        match wanted.filter(|w| doc.page(*w).is_some()) {
            Some(page) => {
                self.ui.page = Some(page);
                Ok(())
            }
            None => Err(bad("page.select", "no page has that id")),
        }
    }

    /// The page the drawing area shows: the one chosen if the document still has it, else the first.
    pub fn current_page(&mut self) -> Option<u32> {
        let doc = self.session().document()?;
        let chosen = self.ui.page.filter(|p| doc.page(*p).is_some());
        // The first tab: the first foreground page, or the first page there is when every page is a background.
        let page = chosen.or_else(|| crate::page_tabs::tab_order(doc).first().map(|(id, _)| *id));
        self.ui.page = page;
        page
    }

    /// The size of the current page in inches.
    pub fn current_page_size(&mut self) -> (f64, f64) {
        let Some(page) = self.current_page() else { return DEFAULT_PAGE_INCHES };
        self.session().document().and_then(|d| page_settings(d, page)).map_or(DEFAULT_PAGE_INCHES, |s| s.size)
    }

    fn view_area(&self) -> egui::Rect {
        self.ui.canvas_rect.unwrap_or_else(fallback_area)
    }

    /// Changes the current page's view with `f`, which is given the view and the drawing area. A page with no view yet
    /// starts from Fit.
    pub(crate) fn with_view(&mut self, f: impl FnOnce(&mut View, egui::Rect)) {
        let Some(page) = self.current_page() else { return };
        let area = self.view_area();
        let size = self.current_page_size();
        let view = self.ui.views.entry(page).or_insert_with(|| View::fit_page(area, size));
        f(view, area);
    }

    /// The zoom of the current page, as a fraction.
    pub fn zoom(&self) -> f64 {
        self.ui.page.and_then(|p| self.ui.views.get(&p)).map_or(1.0, |v| v.zoom)
    }

    pub(crate) fn zoom_to(&mut self, zoom: f64, anchor: Option<egui::Pos2>) {
        self.with_view(|v, area| v.zoom_about(area, anchor.unwrap_or(area.center()), zoom.clamp(MIN_ZOOM, MAX_ZOOM)));
    }

    fn zoom_step(&mut self, factor: f64) {
        let zoom = self.zoom();
        self.zoom_to(zoom * factor, None);
    }

    pub(crate) fn fit(&mut self, width: bool) {
        let size = self.current_page_size();
        self.with_view(|v, area| *v = if width { View::fit_width(area, size) } else { View::fit_page(area, size) });
    }
}

fn backstage_page(cmd: &str, page: &str) -> Result<Option<BackstagePage>> {
    if page.eq_ignore_ascii_case("close") {
        return Ok(None);
    }
    BackstagePage::from_name(page)
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("no File view page `{}`; the pages are Home, New, Print, or close", shown(page, 32))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_command_has_a_parameter_doc_the_engine_can_read() {
        for c in UI_COMMANDS {
            param_keys(c.params).unwrap_or_else(|e| panic!("{}: {e}", c.id));
        }
        let mut ids: Vec<_> = UI_COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), UI_COMMANDS.len(), "an id is listed twice");
    }

    /// A label being typed is sent before a shape operation changes the drawing, as it is before every command that changes it.
    #[test]
    fn a_shape_operation_ends_the_text_edit_that_is_open() {
        for op in crate::operations::OPERATIONS {
            assert!(ends_text_edit(crate::operations::command_of(op)), "{}", crate::operations::command_of(op));
        }
        assert!(!ends_text_edit("view.zoomIn"), "zoom leaves the box open");
    }

    #[test]
    fn no_window_command_shares_an_id_with_an_engine_command() {
        for c in UI_COMMANDS {
            assert!(find_command(c.id).is_none(), "{} is in both registries", c.id);
        }
    }
}
