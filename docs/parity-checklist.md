# Parity checklist

> **Last reviewed:** 2026-11-22

The target is the reference app's desktop edition. The reference app was not inspected, nothing was launched or measured, and our output has not been opened in it by the project. This is the backlog: one row for each row of the feature inventories in `/` (`inventory-*.md`), with the inventory's area, feature name and priority, scored by what the code does on the date above. Feature rows are scored D (done), P (partial) or M (missing), and weighted P0, P1 or P2. The summary comes from `cargo xtask parity`: D counts 1, P counts a half, M counts 0, and a P0 weighs 3, a P1 2 and a P2 1.

**How a row is scored.**

* **D**: an engine command does what the row's grade says and, for a feature a person uses in the window, a window control or a key sends it. A dialog or a second key that is only another way to the same function may be missing; a part of the behaviour the grade names may not.
* **P**: part of it. [`parity-evidence.md`](parity-evidence.md) says what is missing, in a line: the engine has it and the window does not wire it (engine only), the window has the control and the engine lacks the behaviour, or a named part of the behaviour is absent.
* **M**: otherwise. A P2 row is M unless the evidence page says more.
* The window's clipboard, group and stencil controls were wired in gap U20, after a change had left them drawn and quiet, and their rows kept the scores they had when they counted as engine only (P) until the rows were read again on 2026-11-21. Cut, copy and paste, Duplicate by Ctrl-drag and Nested groups are D. Group and ungroup and Open a stencil from More Shapes are D from the Size & Position pane sizes a group, and More Shapes opens every built-in stencil. Duplicate command, Close and reorder stencils and Copy shapes between pages and files stay P, for the parts the evidence page names.
* Drawing what an opened file holds (themes, background pages, layers, pictures) belongs to the viewer and is not a row; a row that asks for the act (apply a theme, assign a layer, insert a picture) is scored on that act.
* Scores err low. A row is D only for what is there and works, not for a menu item that exists.
* **Out of scope by policy** ([roadmap](roadmap.md#out-of-scope-by-policy)): running VBA, the Visual Basic editor, recording VBA macros and COM add-ins stay M and are not planned; a file's VBA project is kept and never run.

**Numbers.** The counts and percentages below are **measured** (`cargo xtask parity` over the table). Each D, P or M is **estimated**: a reading of the command registry, the window's source and tests, and the docs on 2026-11-21, not a run against the reference app and not a test of each row. Evidence for the rows scored D or P is in [`parity-evidence.md`](parity-evidence.md).

The approved application icon is in the OS window decoration, with no duplicate in the app's top bar; the File Home cover remains. The **Backstage / Start screen** row stays M: artwork does not implement start-screen behaviour or recent drawings. No inventory score changes.

The first additional stencil batch (2026-11-12) moved the Gane-Sarson notation row from M to P. The rounded total then remained 20%; the roadmap and target summary were synchronized to that measured checklist value. A later reading (2026-11-21) moves five rows (see the revision history) and the weighted figure from 19.96% to 20.18%, which still rounds to 20%, so those summaries stand.

The accepted stencil presentation is a compact chooser, fixed square icon cards and name-only tooltips. Its filter covers only the current stencil, and Enter in the box (or its magnifier) searches every stencil, which is what the Search for shapes row scores: that row and Quick Shapes are D, and Shape search options is P.

**Scheduled next** (owner, 2026-11-25), after the 0.3 items, in the order the owner listed them ([roadmap](roadmap.md#current-focus)). None is designed or started, and each row keeps its score until the work lands:

1. Connectors that route around shapes, and are routed again when a shape moves: under Connectors: Routing, Automatic reroute on moving shapes (P0, P, the one open P0 item) and Route around placeable shapes (P1, M).
2. The ShapeSheet window, with a command that sets any cell: the rows under ShapeSheet: Opening a sheet, Reading a sheet and Editing formulas (all M).
3. Opening draw.io files and their compressed form: no row of the inventories names it, and the Common diagram formats row of [`file-format-parity.md`](file-format-parity.md) tracks it.
4. DiaCraft's own UML, BPMN, C4, network and database stencils, drawn by the project with no copied art: they give shapes to the rows under Diagram kinds: UML, Networks and cloud and Database diagrams and to the BPMN rows under Diagram kinds: Flowcharts (all M but the Gane-Sarson row, which is P), and a stencil alone makes none of those rows done; no row names C4.

<!-- SUMMARY -->
**Weighted breadth parity: 34%** over 977 features.

| Priority | Features | Done | Partial | Missing | Parity |
|---|---|---|---|---|---|
| P0 | 56 | 55 | 1 | 0 | 99% |
| P1 | 296 | 103 | 58 | 135 | 45% |
| P2 | 625 | 25 | 31 | 569 | 6% |

| Area | Parity |
|---|---|
| Document | 63% |
| Shapes | 62% |
| Connectors | 53% |
| ShapeSheet | 0% |
| Styling | 66% |
| Diagram kinds | 11% |
| Data | 14% |
| Process | 0% |
| Automation | 11% |
| Review | 24% |
| Backstage | 23% |
| Cloud | 0% |

Open P0 items: Automatic reroute on moving shapes.

## Rows

### Document: Pages and page tabs

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Add a page from the page tabs | P1 | D |
| Document | Insert a page with chosen properties | P1 | P |
| Document | Delete a page | P1 | D |
| Document | Rename a page | P1 | D |
| Document | Reorder pages by dragging a tab | P1 | D |
| Document | Reorder pages with renumbering | P1 | D |
| Document | Duplicate a page | P1 | D |
| Document | Move between pages | P1 | D |
| Document | Drag shapes to another page tab | P1 | M |
| Document | Copy shapes between pages and files | P1 | P |

### Document: Background pages

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Create a background page | P1 | D |
| Document | Turn a foreground page into a background | P2 | D |
| Document | Assign a background to a page | P1 | D |
| Document | Background shapes are read-only from the foreground | P1 | D |
| Document | Backgrounds gallery | P1 | M |
| Document | Borders and Titles gallery | P1 | M |
| Document | New page shares the latest background | P2 | M |
| Document | Shared background takes the largest page size | P2 | M |
| Document | Watermark on a background page | P2 | M |
| Document | Theme on a border page reaches its drawing page | P2 | M |

### Document: Page setup and drawing scale

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Page size independent of printer paper | P1 | P |
| Document | Page size presets and custom size | P0 | D |
| Document | Page orientation | P0 | D |
| Document | Auto Size extends the page | P0 | D |
| Document | Fit page to drawing | P1 | M |
| Document | Resize the page by dragging its edge | P1 | M |
| Document | Drawing scale presets and custom ratio | P1 | D |
| Document | Measurement units per page | P1 | M |
| Document | Page rotation by handle | P2 | M |
| Document | View as Horizontal and View as Vertical | P2 | M |
| Document | Center the drawing on the page | P2 | M |

### Document: Grid, rulers, guides and snapping

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Show or hide the grid | P0 | D |
| Document | Variable grid density | P1 | P |
| Document | Fixed grid spacing | P2 | P |
| Document | Grid origin | P2 | P |
| Document | Show or hide rulers | P1 | D |
| Document | Ruler position markers | P2 | M |
| Document | Ruler subdivisions | P2 | P |
| Document | Ruler zero point | P2 | P |
| Document | Show or hide guides | P1 | D |
| Document | Create and delete guides | P1 | P |
| Document | Guide points | P2 | M |
| Document | Glue shapes to guides | P1 | P |
| Document | Snap on or off | P0 | D |
| Document | Choice of snap targets | P1 | P |
| Document | Snap strength | P2 | M |
| Document | Shape extension lines | P2 | M |
| Document | Isometric angle lines | P2 | M |
| Document | Page breaks shown on the page | P1 | D |

### Document: Layers

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Create, rename and delete layers | P1 | D |
| Document | Assign shapes to layers | P1 | D |
| Document | Active layers | P1 | D |
| Document | Layer visibility | P1 | D |
| Document | Layer printing | P1 | D |
| Document | Layer lock | P1 | D |
| Document | Layer snap and glue | P2 | D |
| Document | Layer colour and transparency | P2 | D |
| Document | Remove unreferenced layers | P2 | D |
| Document | Layers come with shapes | P2 | M |

### Document: Viewing and windows

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Scroll and zoom with the wheel | P0 | D |
| Document | Zoom controls and keys | P0 | D |
| Document | Zoom and pan by Ctrl+Shift | P1 | M |
| Document | Pan & Zoom window | P1 | P |
| Document | Touch pinch and drag | P2 | P |
| Document | Task panes show, hide and float | P1 | P |
| Document | New window on the same drawing | P2 | M |
| Document | Arrange, cascade and switch windows | P2 | M |
| Document | Right-to-left view direction | P2 | M |
| Document | Drawing Explorer window | P2 | P |

### Document: Undo and protection

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Multi-level undo and redo | P0 | D |
| Document | Protect Document | P2 | M |

### Document: Printing

| Area | Feature | Priority | Status |
|---|---|---|---|
| Document | Print a drawing | P0 |D |
| Document | Print range | P1 |P |
| Document | Fit current view to one sheet | P2 | M |
| Document | Print preview | P1 |P |
| Document | Tiling across sheets | P1 |D |
| Document | Print zoom Adjust to | P1 |P |
| Document | Print zoom Fit to sheets | P1 |P |
| Document | Printer paper size and orientation | P1 |D |
| Document | Print margins and centring | P2 |P |
| Document | Print gridlines | P2 | M |
| Document | Printed headers and footers | P1 | M |
| Document | Print without the background | P2 | M |
| Document | Non-printing shapes | P2 |P |
| Document | Background printing | P2 |P |

### Shapes: Shapes window and stencils

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | [Shapes window with stacked stencils](ui-parity.md#stencil-gallery) | P0 | D |
| Shapes | Drag a master onto the page | P0 | D |
| Shapes | Insert a selected master by keyboard | P2 | M |
| Shapes | Stamp tool | P2 | M |
| Shapes | Minimize the Shapes window | P1 | P |
| Shapes | Open a stencil from More Shapes | P0 | D |
| Shapes | Close and reorder stencils | P1 | P |
| Shapes | Search for shapes | P1 | D |
| Shapes | Shape search options | P2 | P |
| Shapes | Search result becomes a document master | P2 | M |
| Shapes | Online shape search and download | P2 | M |
| Shapes | Quick Shapes | P1 | D |
| Shapes | My Shapes folder and Favorites | P1 | M |
| Shapes | Stencil view modes | P2 | M |

### Shapes: Masters and custom stencils

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Create a custom stencil | P1 | M |
| Shapes | Edit a stencil | P1 | M |
| Shapes | Add a master by dragging a shape to a stencil | P1 | M |
| Shapes | Edit a master | P1 | M |
| Shapes | Instances inherit from their master | P0 | D |
| Shapes | Rename and delete masters | P1 | M |
| Shapes | Edit a master's icon | P2 | M |
| Shapes | Document stencil | P1 | M |
| Shapes | Master Explorer | P2 | M |
| Shapes | Drop shape resizes to the drawing scale | P1 | M |
| Shapes | Share a stencil file | P2 | M |

### Shapes: Selecting shapes

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Select a shape by clicking | P0 | D |
| Shapes | Add to and remove from a selection | P0 | D |
| Shapes | Primary shape in a multiple selection | P1 | D |
| Shapes | Area selection | P0 | D |
| Shapes | Lasso selection | P1 | D |
| Shapes | Select partly enclosed shapes | P2 | D |
| Shapes | Select all | P0 | D |
| Shapes | Select by Type | P1 | D |
| Shapes | Clear the selection | P0 | D |
| Shapes | Keyboard focus and selection | P1 | M |

### Shapes: Moving, sizing and turning shapes

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Move by dragging | P0 | D |
| Shapes | Nudge with arrow keys | P1 | D |
| Shapes | Move Shapes by distance | P2 | P |
| Shapes | Resize with handles | P0 | D |
| Shapes | Resize a 1-D shape by its endpoints | P0 | D |
| Shapes | Size & Position window | P1 | D |
| Shapes | Units and arithmetic in Size & Position | P1 | D |
| Shapes | Pin position | P1 | P |
| Shapes | Rotate by handle | P1 | D |
| Shapes | Rotate by typed angle | P1 | D |
| Shapes | Rotate 90 and flip | P1 | D |
| Shapes | Rotate or flip a whole diagram | P2 | M |
| Shapes | Duplicate by Ctrl-drag | P1 | D |
| Shapes | Cut, copy and paste | P0 | D |
| Shapes | Duplicate command | P1 | P |
| Shapes | Paste as hyperlink | P2 | M |
| Shapes | Paste Special | P1 | M |
| Shapes | Delete | P0 | D |
| Shapes | Change Shape | P1 | D |
| Shapes | Control handles | P1 | M |
| Shapes | Shape-specific menu commands | P1 | M |
| Shapes | Mini toolbar on right-click | P1 | M |
| Shapes | Touch tap sequence | P2 | M |
| Shapes | Touch to add shapes | P2 | M |
| Shapes | Live preview from galleries | P1 | M |

### Shapes: Arrange

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Align shapes | P0 | D |
| Shapes | Auto Align | P1 | M |
| Shapes | Auto Align and Space | P1 | M |
| Shapes | Auto Space and Spacing Options | P1 | P |
| Shapes | Distribute shapes | P1 | M |
| Shapes | Distribute with guides | P2 | M |
| Shapes | Array shapes | P2 | M |
| Shapes | Move off Page Breaks | P2 | D |
| Shapes | Dynamic grid | P1 | P |
| Shapes | Bring to front and send to back | P0 | D |
| Shapes | Bring forward and send backward | P1 | D |
| Shapes | Group and ungroup | P0 | D |
| Shapes | Add to and remove from a group | P1 | M |
| Shapes | Nested groups | P1 | D |
| Shapes | Convert to Group | P2 | M |
| Shapes | Select a shape inside a group | P1 | D |
| Shapes | Group behaviour settings | P2 | M |

### Shapes: Drawing tools and geometry

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Rectangle and ellipse tools | P1 | D |
| Shapes | Line tool | P1 | D |
| Shapes | Build a shape from segments | P1 | D |
| Shapes | Pencil tool | P1 | P |
| Shapes | Arc tool | P1 | D |
| Shapes | Freeform tool | P1 | D |
| Shapes | Edit vertices | P1 | M |
| Shapes | Close a path into a filled shape | P1 | D |
| Shapes | 1-D and 2-D shape behaviour | P0 | D |
| Shapes | Union | P1 | D |
| Shapes | Combine | P1 | D |
| Shapes | Fragment | P1 | D |
| Shapes | Intersect | P1 | D |
| Shapes | Subtract | P1 | D |
| Shapes | Join | P2 | M |
| Shapes | Trim | P2 | M |
| Shapes | Offset | P2 | M |
| Shapes | Fit Curve | P2 | M |
| Shapes | Update Alignment Box | P2 | M |
| Shapes | Corner rounding | P1 | P |
| Shapes | Measure tool shapes | P2 | M |
| Shapes | Dimension shapes | P2 | M |

### Shapes: Shape properties

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Shape protection locks | P1 | M |
| Shapes | Lock from selection | P2 | M |
| Shapes | Double-click action | P2 | M |
| Shapes | Name a shape | P2 | M |
| Shapes | ScreenTip on a shape | P1 | M |
| Shapes | Hyperlinks on a shape | P1 | D |
| Shapes | Follow a hyperlink | P1 | D |
| Shapes | Embed an object from another program | P2 | M |

### Shapes: Text

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Type text on a selected shape | P0 | D |
| Shapes | Double-click to edit text | P0 | D |
| Shapes | Auto zoom while editing small text | P1 | M |
| Shapes | Text tool and text boxes | P0 | D |
| Shapes | Vertical text box | P2 | P |
| Shapes | Text Block tool | P1 | D |
| Shapes | Rotate text | P1 | D |
| Shapes | Font and size | P0 | D |
| Shapes | Font colour | P0 | D |
| Shapes | Bold, italic and underline | P0 | D |
| Shapes | Double underline | P2 | M |
| Shapes | Strikethrough | P2 | D |
| Shapes | Case changes | P1 | D |
| Shapes | Subscript and superscript | P1 | M |
| Shapes | Font width scale | P2 | M |
| Shapes | Overline | P2 | M |
| Shapes | Horizontal paragraph alignment | P0 | D |
| Shapes | Vertical text alignment | P1 | D |
| Shapes | Indents and paragraph spacing | P1 | P |
| Shapes | Text ruler | P1 | M |
| Shapes | Bulleted lists | P1 | D |
| Shapes | Tab stops | P1 | M |
| Shapes | Text block margins and text background | P2 | M |
| Shapes | Right-to-left text direction | P2 | D |
| Shapes | Text fields | P1 | P |
| Shapes | Insert symbol | P2 | D |
| Shapes | Equation Editor object | P2 | M |
| Shapes | Find text | P1 | P |
| Shapes | Replace text | P1 | P |

### Shapes: Callouts and containers

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Add a callout | P1 | P |
| Shapes | Callout follows its shape | P1 | D |
| Shapes | Callout shapes from stencils | P2 | M |
| Shapes | Add a container | P1 | D |
| Shapes | Container membership by drop | P1 | D |
| Shapes | Members move, copy and delete with the container | P1 | D |
| Shapes | Attach a shape to a container edge | P2 | P |
| Shapes | Container style and margins | P1 | D |
| Shapes | Container fit and auto resize | P1 | D |
| Shapes | Lock a container | P2 | D |
| Shapes | Disband a container | P1 | D |
| Shapes | Select container contents | P2 | D |
| Shapes | Connect to a container | P2 | P |
| Shapes | Container heading position | P2 | P |
| Shapes | List shapes | P2 | M |

### Shapes: Pictures and inserted objects

| Area | Feature | Priority | Status |
|---|---|---|---|
| Shapes | Insert a picture from file | P1 | P |
| Shapes | Stock images, icons and online pictures | P2 | M |
| Shapes | Move, resize and format a picture | P1 | P |
| Shapes | Crop a picture | P1 | M |
| Shapes | Picture brightness and contrast | P2 | M |
| Shapes | Picture image controls | P2 | M |
| Shapes | Picture AutoBalance | P2 | M |
| Shapes | Compress pictures | P2 | M |
| Shapes | Insert a chart | P2 | M |
| Shapes | Tables from shapes | P2 | M |
| Shapes | Insert a CAD drawing | P2 | M |

### Connectors: Drawing connectors

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Connector tool | P0 | D |
| Connectors | Connect Shapes command | P1 | P |
| Connectors | AutoConnect arrows and flyout | P0 | D |
| Connectors | AutoConnect drag to an existing shape | P1 | D |
| Connectors | Drop a master on an AutoConnect arrow | P1 | D |
| Connectors | AutoConnect toggle at two levels | P1 | P |
| Connectors | Which shapes the AutoConnect flyout offers | P2 | P |
| Connectors | Alternative connector shapes | P2 | M |
| Connectors | Connector control on the Insert tab | P2 | M |

### Connectors: Glue and connection points

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Point-to-point glue | P0 | D |
| Connectors | Shape-to-shape glue | P0 | D |
| Connectors | Mixed glue on one connector | P1 | D |
| Connectors | Glue master switch and targets | P1 | P |
| Connectors | Glued and unglued end markers | P1 | D |
| Connectors | Unglue one end by dragging | P0 | D |
| Connectors | Unglue both ends with an arrow key | P2 | M |
| Connectors | Connector to connector | P2 | M |
| Connectors | Show connection points | P1 | P |
| Connectors | Add, move and delete connection points | P1 | M |
| Connectors | Inward, outward and both point types | P2 | M |
| Connectors | Glue to the shape outline or its alignment box | P2 | M |
| Connectors | Which side a shape-glued 1-D shape takes | P2 | M |

### Connectors: Routing

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Right-angle connectors | P0 | D |
| Connectors | Straight connectors | P1 | P |
| Connectors | Curved connectors | P1 | P |
| Connectors | Default connector type for a page | P1 | P |
| Connectors | Routing style per page and connector | P1 | M |
| Connectors | Automatic reroute on moving shapes | P0 | P |
| Connectors | Route around placeable shapes | P1 | M |
| Connectors | Reset Connector | P1 | D |
| Connectors | Edit a connector path by hand | P0 | D |
| Connectors | Reverse Ends | P1 | M |
| Connectors | Connector splits when a shape drops on it | P1 | M |
| Connectors | Healing a connector when a shape is deleted | P1 | M |
| Connectors | Move shapes away on drop | P2 | M |
| Connectors | Page spacing for routing | P2 | M |

### Connectors: Line jumps

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Show line jumps | P1 | P |
| Connectors | Line jump style | P2 | P |
| Connectors | Which connector carries the jump | P2 | P |
| Connectors | Line jump size | P2 | P |
| Connectors | Line jump direction on a connector | P2 | P |

### Connectors: Connector text

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Text on a connector | P0 | D |
| Connectors | Move connector text along the line | P1 | M |
| Connectors | Save a custom connector | P2 | M |
| Connectors | Connector tool uses the selected master | P1 | M |

### Connectors: Automatic layout

| Area | Feature | Priority | Status |
|---|---|---|---|
| Connectors | Re-Layout Page | P1 | P |
| Connectors | Layout placement, flip and spacing options | P2 | P |
| Connectors | Layout on a selection | P1 | M |
| Connectors | Exclude a shape from layout | P2 | M |
| Connectors | Avoid Page Breaks option | P2 | D |

### ShapeSheet: Opening a sheet

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Open the sheet of a shape, page or document | P1 | M |
| ShapeSheet | Sheets of other objects | P2 | M |
| ShapeSheet | Run in developer mode | P2 | M |
| ShapeSheet | Sheet stays on the object it opened with | P2 | M |
| ShapeSheet | Open each sheet in the same window | P2 | M |

### ShapeSheet: Reading a sheet

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Sections, rows and cells in a sheet window | P1 | M |
| ShapeSheet | Drawing and sheet update together | P2 | M |
| ShapeSheet | Selecting a vertex cell marks it on the drawing | P2 | M |
| ShapeSheet | Show formulas or values | P1 | M |
| ShapeSheet | Local and inherited cell colours | P2 | M |
| ShapeSheet | Error values in cells | P2 | M |
| ShapeSheet | Move around a sheet with the keyboard | P2 | M |
| ShapeSheet | Show or hide sections | P2 | M |
| ShapeSheet | Collapse or expand a section | P2 | M |

### ShapeSheet: Editing formulas

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Edit in the cell or the formula bar | P1 | M |
| ShapeSheet | Type a new formula | P1 | M |
| ShapeSheet | Empty formula returns to inheriting | P1 | M |
| ShapeSheet | Reference a cell by clicking it | P2 | M |
| ShapeSheet | Copy a formula between cells | P2 | M |
| ShapeSheet | Formula AutoComplete | P2 | M |
| ShapeSheet | Edit a long formula in a larger editor | P2 | M |
| ShapeSheet | Insert a function from a list | P2 | M |
| ShapeSheet | Insert a cell name from a list | P2 | M |

### ShapeSheet: Sections and rows

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Insert a section | P2 | M |
| ShapeSheet | Delete a section | P2 | M |
| ShapeSheet | Insert a row | P2 | M |
| ShapeSheet | Delete a row | P2 | M |
| ShapeSheet | Change a row's type | P2 | M |
| ShapeSheet | Expand a row | P2 | M |
| ShapeSheet | Row commands for shape data, hyperlinks and actions | P2 | M |

### ShapeSheet: Tracing formulas

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Formula Tracing window | P2 | M |
| ShapeSheet | Trace dependent cells | P2 | M |
| ShapeSheet | Trace precedent cells | P2 | M |
| ShapeSheet | Go to a traced cell | P2 | M |
| ShapeSheet | Trace back and forward | P2 | M |

### ShapeSheet: Style Explorer

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Style Explorer window | P2 | M |
| ShapeSheet | Show which cells inherit from a style | P2 | M |
| ShapeSheet | Show which style a cell inherits from | P2 | M |

### ShapeSheet: Action tags and shape identity

| Area | Feature | Priority | Status |
|---|---|---|---|
| ShapeSheet | Action tags on a shape | P2 | M |
| ShapeSheet | Turn action tags on or off | P2 | M |
| ShapeSheet | Show a shape's ID | P2 | M |

### Styling: Themes

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Apply a theme | P0 | D |
| Styling | Apply a theme to one page or all | P1 | P |
| Styling | Remove a theme | P1 | P |
| Styling | Apply theme to new shapes | P1 | P |
| Styling | Stop a theme reaching a shape | P1 | P |
| Styling | Standard colours stay put | P1 | D |
| Styling | Theme variants | P1 | P |
| Styling | Swap a theme's colours | P1 | M |
| Styling | Swap a theme's effects | P2 | M |
| Styling | Swap a theme's connector style | P2 | M |
| Styling | Theme embellishment | P2 | M |
| Styling | Custom theme colours | P2 | M |
| Styling | Text stays visible against a dark background | P2 | M |
| Styling | Theme background colour | P2 | M |

### Styling: Quick Styles and format copying

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Apply a Quick Style | P1 | D |
| Styling | Format Painter | P1 | D |
| Styling | Format Painter on many shapes | P1 | D |

### Styling: Fill

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Fill colour | P0 | D |
| Styling | Custom colours by RGB or hex | P1 | D |
| Styling | Fill transparency | P1 | D |
| Styling | Gradient fill | P1 | D |
| Styling | Gradient rotates with the shape | P2 | D |
| Styling | Pattern fill | P1 | D |
| Styling | Custom fill, line and line-end patterns | P2 | M |
| Styling | Color Palette dialog | P2 | M |

### Styling: Line

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Line colour | P0 | D |
| Styling | Line weight | P0 | D |
| Styling | Line dash style | P1 | D |
| Styling | Arrowheads on lines | P0 | D |
| Styling | Line cap | P2 | D |
| Styling | Compound line | P2 | D |
| Styling | Line transparency | P1 | D |
| Styling | Gradient line | P2 | D |
| Styling | Format ink | P2 | M |

### Styling: Effects

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Format Shape pane with live preview | P1 | D |
| Styling | Drop shadow | P1 | D |
| Styling | Page shadow offset | P2 | M |
| Styling | Reflection | P2 | M |
| Styling | Glow | P2 | M |
| Styling | Soft edges | P2 | M |
| Styling | Bevel | P2 | M |
| Styling | 3-D rotation | P2 | M |
| Styling | Hand-drawn themes and sketch effect | P2 | M |

### Styling: Named styles

| Area | Feature | Priority | Status |
|---|---|---|---|
| Styling | Apply a named style | P2 | M |
| Styling | Define a named style | P2 | M |

### Diagram kinds: Starting from a template

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Start a drawing from a template | P0 | D |
| Diagram kinds | Template opens its stencils | P0 | D |
| Diagram kinds | Template sets page scale, size and orientation | P1 | P |
| Diagram kinds | Choose metric or US units when starting | P1 | M |
| Diagram kinds | Browse templates by category | P1 | P |
| Diagram kinds | Search for a template | P1 | M |
| Diagram kinds | Template adds its own ribbon tab | P1 | M |
| Diagram kinds | Starter diagrams beside the blank template | P2 | M |
| Diagram kinds | Sample diagrams for the common kinds | P2 | M |
| Diagram kinds | Save a drawing as a custom template | P1 | P |
| Diagram kinds | Start a drawing from a custom template | P1 | P |
| Diagram kinds | Partner-created templates and diagrams | P2 | M |

### Diagram kinds: Flowcharts

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Basic Flowchart template | P0 | D |
| Diagram kinds | Off-page reference links two pages | P1 | M |
| Diagram kinds | Number Shapes | P2 | M |
| Diagram kinds | Cross-Functional Flowchart template | P1 | D |
| Diagram kinds | Swimlane orientation | P1 | P |
| Diagram kinds | Insert a swimlane | P1 | D |
| Diagram kinds | Swimlane holds its shapes | P1 | D |
| Diagram kinds | Move and resize swimlanes | P1 | P |
| Diagram kinds | Swimlane label text and direction | P1 | D |
| Diagram kinds | Phase separators | P1 | D |
| Diagram kinds | Work Flow Diagram template | P2 | M |
| Diagram kinds | BPMN Diagram template | P1 | M |
| Diagram kinds | BPMN shape variants by right-click | P1 | M |
| Diagram kinds | BPMN connector types | P1 | M |
| Diagram kinds | BPMN refuses non-standard styling | P2 | M |
| Diagram kinds | BPMN workflow starter templates | P2 | M |
| Diagram kinds | SDL Diagram template | P2 | M |
| Diagram kinds | IDEF0 Diagram template | P2 | M |
| Diagram kinds | IDEF0 title block and activity boxes | P2 | M |
| Diagram kinds | IDEF0 arrow join, fork, branch and tunnel | P2 | M |
| Diagram kinds | IDEF0 node trees | P2 | M |
| Diagram kinds | Microsoft SharePoint Workflow template | P2 | M |

### Diagram kinds: Process methods

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Audit Diagram template | P2 | M |
| Diagram kinds | Event-driven Process Chain template | P2 | M |
| Diagram kinds | Fault Tree Analysis template | P2 | M |
| Diagram kinds | ITIL Diagram template | P2 | M |
| Diagram kinds | Six Sigma Diagram template | P2 | M |
| Diagram kinds | House of Quality matrix | P2 | M |
| Diagram kinds | TQM Diagram template | P2 | M |
| Diagram kinds | Value Stream Map template | P2 | M |
| Diagram kinds | Cause and Effect Diagram template | P2 | M |

### Diagram kinds: Schedules

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Calendar template | P1 | M |
| Diagram kinds | Generate a month or day calendar with dates | P1 | M |
| Diagram kinds | Generate a week or multi-week calendar | P2 | M |
| Diagram kinds | Generate a year calendar | P2 | M |
| Diagram kinds | Calendar appointment shapes | P2 | M |
| Diagram kinds | Calendar multi-day events | P2 | M |
| Diagram kinds | Calendar day markers and text | P2 | M |
| Diagram kinds | Thumbnail month shape | P2 | M |
| Diagram kinds | Timeline template | P1 | M |
| Diagram kinds | Timeline bar with dates and scale | P1 | M |
| Diagram kinds | Timeline date formats and markings | P2 | M |
| Diagram kinds | Timeline milestones | P1 | M |
| Diagram kinds | Timeline intervals | P1 | M |
| Diagram kinds | Timeline text moves off the bar with a bracket | P2 | M |
| Diagram kinds | Today marker and elapsed time update themselves | P2 | M |
| Diagram kinds | Expanded timeline | P2 | M |
| Diagram kinds | Synchronized milestones and intervals | P2 | M |
| Diagram kinds | Change timeline, milestone and interval style | P2 | M |
| Diagram kinds | Import a Project file into a timeline | P2 | M |
| Diagram kinds | Export a timeline to a Project file | P2 | M |
| Diagram kinds | Gantt Chart template | P1 | M |
| Diagram kinds | Gantt timescale range and units | P2 | M |
| Diagram kinds | Gantt task rows and durations | P1 | M |
| Diagram kinds | Gantt milestones as zero-length tasks | P2 | M |
| Diagram kinds | Gantt summary tasks by indent | P2 | M |
| Diagram kinds | Gantt task dependencies | P1 | M |
| Diagram kinds | Gantt percent complete | P2 | M |
| Diagram kinds | Gantt bar and marker styles | P2 | M |
| Diagram kinds | Gantt data columns | P2 | M |
| Diagram kinds | Gantt working time | P2 | M |
| Diagram kinds | Gantt timescale navigation | P2 | M |
| Diagram kinds | Gantt frame and timescale width | P2 | M |
| Diagram kinds | Build a Gantt chart from a data file | P2 | M |
| Diagram kinds | PERT Chart template | P2 | M |
| Diagram kinds | PERT hierarchical summary shape | P2 | M |

### Diagram kinds: Organization charts

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Organization Chart template | P1 | D |
| Diagram kinds | Drop a shape on its manager | P1 | P |
| Diagram kinds | Change a position's type | P2 | M |
| Diagram kinds | Org chart layout for a whole chart | P1 | P |
| Diagram kinds | Arrange one manager's subordinates | P1 | P |
| Diagram kinds | Re-layout an org chart | P1 | D |
| Diagram kinds | Fit an org chart to the page | P2 | M |
| Diagram kinds | Org chart spacing | P2 | M |
| Diagram kinds | Show or hide subordinates | P2 | M |
| Diagram kinds | Move a report left or right | P2 | M |
| Diagram kinds | Org chart shape styles | P2 | M |
| Diagram kinds | Org chart shape size controls | P2 | M |
| Diagram kinds | Org chart picture on a shape | P1 | P |
| Diagram kinds | Org chart pictures from a folder | P2 | M |
| Diagram kinds | Fields shown on org chart shapes | P2 | M |
| Diagram kinds | Team frame and dotted-line reports | P2 | M |
| Diagram kinds | Org chart wizard from a data file | P1 | M |
| Diagram kinds | Org chart wizard from a database or directory | P2 | M |
| Diagram kinds | Org chart wizard makes a sample data file | P2 | M |
| Diagram kinds | Org chart wizard field choices | P2 | M |
| Diagram kinds | Org chart across several pages | P2 | M |
| Diagram kinds | Hyperlink a person across pages | P2 | M |
| Diagram kinds | Synchronized copies of a person | P2 | M |
| Diagram kinds | Refresh an org chart from its data | P2 | M |
| Diagram kinds | Compare two versions of an org chart | P2 | M |
| Diagram kinds | Export org chart data | P2 | M |

### Diagram kinds: Brainstorming and mind maps

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Brainstorming Diagram template | P1 | M |
| Diagram kinds | Add main topic, subtopic and peer | P1 | M |
| Diagram kinds | Add multiple subtopics at once | P2 | M |
| Diagram kinds | Auto-arrange topics | P1 | M |
| Diagram kinds | Outline window for topics | P2 | M |
| Diagram kinds | Re-parent topics in the outline | P2 | M |
| Diagram kinds | Reorder topics in the outline only | P2 | M |
| Diagram kinds | Move a topic to a new page | P2 | M |
| Diagram kinds | Association line between topics | P2 | M |

### Diagram kinds: Charts and marketing shapes

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Charts and Graphs template | P2 | M |
| Diagram kinds | Bar graph shapes | P2 | M |
| Diagram kinds | 3-D bar graph and axis | P2 | M |
| Diagram kinds | Line graph shape | P2 | M |
| Diagram kinds | Pie chart shape | P2 | M |
| Diagram kinds | Pie slice shapes for an emphasized slice | P2 | M |
| Diagram kinds | Graph axis labels and annotations | P2 | M |
| Diagram kinds | Marketing Charts and Diagrams template | P2 | M |
| Diagram kinds | Circle-spoke shape | P2 | M |
| Diagram kinds | Market analysis and marketing mix shapes | P2 | M |
| Diagram kinds | Matrix shape | P2 | M |
| Diagram kinds | Pyramid and triangle shapes | P2 | M |
| Diagram kinds | Venn diagram shape | P2 | M |

### Diagram kinds: General diagrams

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Basic Diagram template | P0 | D |
| Diagram kinds | Block Diagram template | P1 | M |
| Diagram kinds | Open-ended blocks that flow together | P2 | M |
| Diagram kinds | Tree shapes in block diagrams | P2 | M |
| Diagram kinds | Concentric layers for onion diagrams | P2 | M |
| Diagram kinds | Block Diagram With Perspective template | P2 | M |
| Diagram kinds | Second vanishing point | P2 | M |
| Diagram kinds | Depth of a perspective block | P2 | M |
| Diagram kinds | Business Matrix diagrams | P2 | M |
| Diagram kinds | Cycle diagrams | P2 | M |
| Diagram kinds | Pyramid diagrams | P2 | M |
| Diagram kinds | Venn Diagram template | P2 | M |
| Diagram kinds | Graph and Math Shapes stencil | P2 | M |
| Diagram kinds | Infographic, sticky note and icon stencils | P2 | M |
| Diagram kinds | Three-dimensional stencils and templates | P2 | M |

### Diagram kinds: PivotDiagram

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | PivotDiagram template | P2 | M |
| Diagram kinds | Break a node down by category | P2 | M |
| Diagram kinds | PivotDiagram totals | P2 | M |
| Diagram kinds | Filter PivotDiagram nodes | P2 | M |
| Diagram kinds | Limit nodes in a breakdown | P2 | M |
| Diagram kinds | Sort a PivotDiagram breakdown | P2 | M |
| Diagram kinds | Rename PivotDiagram items | P2 | M |
| Diagram kinds | Refresh a PivotDiagram | P2 | M |
| Diagram kinds | PivotDiagram from an analysis cube | P2 | M |

### Diagram kinds: Education

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Algebra template | P2 | M |
| Diagram kinds | Analog Clock template | P2 | M |
| Diagram kinds | Electromagnetism template | P2 | M |
| Diagram kinds | Geometry template | P2 | M |
| Diagram kinds | Molecular Models template | P2 | M |
| Diagram kinds | Music Instruments and Notes template | P2 | M |
| Diagram kinds | Organic Compounds and Equations template | P2 | M |
| Diagram kinds | Planisphere template | P2 | M |
| Diagram kinds | School Posters template | P2 | M |
| Diagram kinds | Science Posters template | P2 | M |
| Diagram kinds | Sets template | P2 | M |
| Diagram kinds | Soccer Formations template | P2 | M |
| Diagram kinds | Tournament Fixtures template | P2 | M |
| Diagram kinds | Trigonometry template | P2 | M |

### Diagram kinds: Networks and cloud

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Basic Network Diagram template | P1 | M |
| Diagram kinds | Detailed Network Diagram template | P1 | M |
| Diagram kinds | Network backbone with built-in connectors | P2 | M |
| Diagram kinds | Legend shape | P2 | M |
| Diagram kinds | Label Shapes add-on | P2 | M |
| Diagram kinds | Rack Diagram template | P2 | M |
| Diagram kinds | Rack units on the rack shape | P2 | M |
| Diagram kinds | Equipment glues into a rack | P2 | M |
| Diagram kinds | LDAP Directory template | P2 | M |
| Diagram kinds | Active Directory template | P2 | M |
| Diagram kinds | Azure Diagrams template | P1 | M |
| Diagram kinds | AWS Diagrams template | P1 | M |
| Diagram kinds | Kubernetes shapes | P2 | M |

### Diagram kinds: Maps and floor plans

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Floor Plan template | P1 | M |
| Diagram kinds | Room shapes | P1 | M |
| Diagram kinds | Walls glue and clean up at intersections | P1 | M |
| Diagram kinds | Doors and windows fit into walls | P1 | M |
| Diagram kinds | Wall fixtures glue to walls | P2 | M |
| Diagram kinds | Dimension a wall and type its size | P2 | M |
| Diagram kinds | Add a plan on top of a floor plan | P2 | M |
| Diagram kinds | Home Plan template | P2 | M |
| Diagram kinds | Office Layout template | P1 | M |
| Diagram kinds | Space Plan template | P2 | M |
| Diagram kinds | Space plan startup steps | P2 | M |
| Diagram kinds | Space Explorer and Category Explorer | P2 | M |
| Diagram kinds | Space shape auto-size | P2 | M |
| Diagram kinds | Import space and resource data | P2 | M |
| Diagram kinds | Space plan display options | P2 | M |
| Diagram kinds | Site Plan template | P2 | M |
| Diagram kinds | Parking and plants glue to guides | P2 | M |
| Diagram kinds | HVAC Control Logic template | P2 | M |
| Diagram kinds | Reflected Ceiling Plan template | P2 | M |
| Diagram kinds | HVAC Plan template | P2 | M |
| Diagram kinds | Duct shapes resize by data and join by size | P2 | M |
| Diagram kinds | Duct size label | P2 | M |
| Diagram kinds | Plumbing and Piping Plan template | P2 | M |
| Diagram kinds | Electrical and Telecom Plan template | P2 | M |
| Diagram kinds | Security and Access Plan template | P2 | M |
| Diagram kinds | Plant Layout template | P2 | M |
| Diagram kinds | Directional Map template | P2 | M |
| Diagram kinds | Road and metro base width | P2 | M |
| Diagram kinds | Directional Map 3D template | P2 | M |
| Diagram kinds | Crime Scene Investigation template | P2 | M |
| Diagram kinds | Social-distancing floor plans | P2 | M |

### Diagram kinds: Engineering

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Basic Electrical template | P2 | M |
| Diagram kinds | Circuits and Logic template | P2 | M |
| Diagram kinds | Industrial Control Systems template | P2 | M |
| Diagram kinds | Systems template | P2 | M |
| Diagram kinds | Fluid Power template | P2 | M |
| Diagram kinds | Part and Assembly Drawing template | P2 | M |
| Diagram kinds | Belt drawing with tangent shapes | P2 | M |
| Diagram kinds | Feature control frames | P2 | M |
| Diagram kinds | Welding symbols | P2 | M |
| Diagram kinds | Piping and Instrumentation Diagram template | P2 | M |
| Diagram kinds | Process Flow Diagram template | P2 | M |
| Diagram kinds | Pipeline connects anywhere on equipment | P2 | M |
| Diagram kinds | Valve splits a pipeline | P2 | M |
| Diagram kinds | Component tag numbers | P2 | M |
| Diagram kinds | Renumber process components | P2 | M |

### Diagram kinds: UML

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | UML Class template | P1 | M |
| Diagram kinds | UML class members and fields | P1 | M |
| Diagram kinds | UML Sequence template | P1 | M |
| Diagram kinds | Sequence lifelines | P1 | M |
| Diagram kinds | Sequence messages | P1 | M |
| Diagram kinds | Sequence fragments | P2 | M |
| Diagram kinds | Sequence activation and destruction | P2 | M |
| Diagram kinds | UML Use Case template | P1 | M |
| Diagram kinds | UML Activity template | P1 | M |
| Diagram kinds | UML Component template | P2 | M |
| Diagram kinds | UML component holds other components | P2 | M |
| Diagram kinds | UML stereotype label on a shape | P2 | M |
| Diagram kinds | UML connector type and multiplicity | P2 | M |
| Diagram kinds | UML Deployment template | P2 | M |
| Diagram kinds | UML State Machine template | P2 | M |
| Diagram kinds | UML Communication template | P2 | M |
| Diagram kinds | UML Database Notation template | P2 | M |

### Diagram kinds: Database diagrams

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Crow's Foot Database Notation template | P1 | M |
| Diagram kinds | Set relationship end symbols | P1 | M |
| Diagram kinds | Entity height follows its attributes | P2 | M |
| Diagram kinds | Show attribute types on an entity | P2 | M |
| Diagram kinds | Chen's Database Notation template | P2 | M |
| Diagram kinds | IDEF1X Database Notation template | P2 | M |
| Diagram kinds | Database Model Diagram template | P2 | M |
| Diagram kinds | Table definition and columns | P2 | M |
| Diagram kinds | Column data types | P2 | M |
| Diagram kinds | Portable and physical data types | P2 | M |
| Diagram kinds | User-defined data types | P2 | M |
| Diagram kinds | Composite types and typed tables | P2 | M |
| Diagram kinds | Collection types on a column | P2 | M |
| Diagram kinds | Relationship creates a foreign key | P2 | M |
| Diagram kinds | Relationship cardinality | P2 | M |
| Diagram kinds | Relationship notation on a database model | P2 | M |
| Diagram kinds | Referential integrity actions | P2 | M |
| Diagram kinds | Database indexes | P2 | M |
| Diagram kinds | Database views | P2 | M |
| Diagram kinds | Check clauses, triggers and code window | P2 | M |
| Diagram kinds | Reverse engineer a database | P2 | M |
| Diagram kinds | Import a modelling tool's file | P2 | M |
| Diagram kinds | Data Flow Diagram template | P2 | M |
| Diagram kinds | Data Flow Model Diagram with Gane-Sarson notation | P2 | P |
| Diagram kinds | Yourdon-Coad notation shapes | P2 | M |
| Diagram kinds | COM and OLE template | P2 | M |
| Diagram kinds | Enterprise Application template | P2 | M |
| Diagram kinds | Program Structure template | P2 | M |

### Diagram kinds: Web and interface design

| Area | Feature | Priority | Status |
|---|---|---|---|
| Diagram kinds | Conceptual Web Site template | P2 | M |
| Diagram kinds | Generate a web site map | P2 | M |
| Diagram kinds | Mark broken links on a site map | P2 | M |
| Diagram kinds | Report broken links | P2 | M |
| Diagram kinds | Compare two site maps | P2 | M |
| Diagram kinds | Wireframe Diagram template | P2 | M |
| Diagram kinds | Wireframe Website template | P2 | M |
| Diagram kinds | Wireframe Mobile App template | P2 | M |
| Diagram kinds | Wireframe containers fill from their items | P2 | M |
| Diagram kinds | Wireframe control states | P2 | M |

### Data: Shape Data

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Shape Data window | P1 | D |
| Data | Enter shape data values | P1 | D |
| Data | Add and delete shape data fields | P1 | D |
| Data | Shape data field types | P1 | D |
| Data | Fixed and variable list fields | P1 | D |
| Data | Field display format | P2 | D |
| Data | Prompt text on a field | P2 | D |
| Data | Ask for data on drop | P2 | P |
| Data | Hidden shape data fields | P2 | D |
| Data | Shape data field order | P2 | D |
| Data | Field language and calendar | P2 | P |
| Data | Shape data sets | P2 | M |
| Data | Apply a data set to shapes | P2 | M |

### Data: Importing external data

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Import an Excel workbook through the wizard | P2 | M |
| Data | Quick Import of a workbook | P2 | M |
| Data | Import an Access database | P2 | M |
| Data | Import a SharePoint list | P2 | M |
| Data | Import a SQL Server table or view | P2 | M |
| Data | Import through OLE DB or ODBC | P2 | M |
| Data | Import a delimited text file | P1 | M |
| Data | Reuse a saved data connection | P2 | M |
| Data | Choose a worksheet or range to import | P2 | M |
| Data | Choose columns and rows to import | P2 | M |
| Data | Filter and sort rows on import | P2 | M |
| Data | Key column for matching rows | P1 | M |
| Data | Several data sources in one drawing | P2 | M |

### Data: External Data window

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | External Data window | P1 | M |
| Data | Rename a data column | P2 | M |
| Data | Reorder data columns | P2 | M |
| Data | Hide a data column | P2 | M |

### Data: Linking rows to shapes

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Link a row to a shape by hand | P1 | M |
| Data | Link rules between rows and shapes | P1 | M |
| Data | Linked columns become shape data | P1 | M |
| Data | Link many rows by matching values | P1 | M |
| Data | Create shapes from rows | P1 | M |
| Data | Find the shape or row at the other end of a link | P2 | M |
| Data | Unlink rows and shapes | P2 | M |

### Data: Refreshing data

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Refresh linked data | P1 | M |
| Data | Refresh on a schedule | P2 | M |
| Data | Offer refresh when a drawing opens | P2 | M |
| Data | Link new rows after a refresh | P2 | M |
| Data | Resolve a deleted-row conflict | P2 | M |
| Data | Resolve a duplicate-key conflict | P2 | M |

### Data: Data Graphics

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Choose fields shown as graphics | P1 | M |
| Data | Data graphic scope by selection | P1 | M |
| Data | Graphics applied on import | P1 | M |
| Data | Data bar graphic | P1 | M |
| Data | Data bar label and value settings | P2 | M |
| Data | Icon set graphic | P1 | M |
| Data | Text callout graphic | P1 | M |
| Data | Color by value | P1 | M |
| Data | Position graphics on many shapes | P2 | M |
| Data | Move one graphic by its handle | P2 | M |
| Data | Data graphic legend | P1 | M |
| Data | Build and edit a data graphic | P2 | M |
| Data | Remove a data graphic | P1 | M |
| Data | Apply a graphic after linking | P2 | M |
| Data | SmartSpace on the Data tab | P2 | M |

### Data: Data Visualizer

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Data Visualizer templates | P2 | M |
| Data | Excel data template from the preview | P2 | M |
| Data | Build a diagram from a table | P2 | M |
| Data | Map functions and phases | P2 | M |
| Data | Map process step columns | P2 | M |
| Data | Map values to shapes | P2 | M |
| Data | Connect steps from data | P2 | M |
| Data | Extra columns become shape data | P2 | M |
| Data | Refresh a diagram from its table | P2 | M |
| Data | Write a diagram back to its table | P2 | M |
| Data | Relink a diagram's workbook | P2 | M |
| Data | Open a diagram's workbook | P2 | M |
| Data | Data Visualizer layouts | P2 | M |
| Data | Export a template package | P2 | M |
| Data | Install a template package | P2 | M |

### Data: Shape reports

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Run a report on shape data | P2 | M |
| Data | Supplied report definitions | P2 | M |
| Data | Report to a web page or XML file | P2 | M |
| Data | Report to an Excel worksheet | P2 | M |
| Data | Report as a shape on the drawing | P2 | M |
| Data | Define a report's scope | P2 | M |
| Data | Choose report columns | P2 | M |
| Data | Report subtotals and grouping | P2 | M |
| Data | Report sort and column order | P2 | M |
| Data | Save a report definition | P2 | M |

### Data: Database export

| Area | Feature | Priority | Status |
|---|---|---|---|
| Data | Export shape data to a database table | P2 | M |
| Data | Choose cells for database export | P2 | M |
| Data | Repeat a database export | P2 | M |
| Data | ODBC retry and monitor settings | P2 | M |
| Data | Link a shape to a database | P2 | M |

### Process: Checking a diagram

| Area | Feature | Priority | Status |
|---|---|---|---|
| Process | Check a diagram against its rules | P1 | M |
| Process | Issues window | P1 | M |
| Process | Rule sets shipped with templates | P1 | M |
| Process | Choose which rule sets are active | P2 | M |
| Process | Import rules into a drawing | P2 | M |
| Process | Ignore one issue | P2 | M |
| Process | Ignore a rule | P2 | M |
| Process | Show ignored issues | P2 | M |
| Process | Stop ignoring an issue or rule | P2 | M |
| Process | Go to a shape from an issue | P2 | M |
| Process | Arrange the Issues window | P2 | M |
| Process | Issues saved with the drawing | P2 | M |
| Process | Write custom validation rules | P2 | M |

### Process: Subprocesses

| Area | Feature | Priority | Status |
|---|---|---|---|
| Process | New subprocess page for a shape | P1 | M |
| Process | Move shapes out into a subprocess | P1 | M |
| Process | Subprocess page keeps swimlanes | P2 | M |
| Process | Link a shape to an existing subprocess | P1 | M |

### Process: SharePoint workflows

| Area | Feature | Priority | Status |
|---|---|---|---|
| Process | Export a SharePoint workflow | P2 | M |
| Process | Import a SharePoint workflow | P2 | M |
| Process | SharePoint workflow rule set | P2 | M |
| Process | Create a workflow from a stage view | P2 | M |
| Process | Create a workflow diagram from the Process tab | P2 | M |
| Process | Process repository on SharePoint | P2 | M |
| Process | Import a cloud flow as a BPMN diagram | P2 | M |

### Process: Documenting a process in Word

| Area | Feature | Priority | Status |
|---|---|---|---|
| Process | Export a page to a Word document | P2 | M |
| Process | Pick and order Word snippets | P2 | M |
| Process | Word export property options | P2 | M |
| Process | Word export shape options | P2 | M |

### Automation: Macros

| Area | Feature | Priority | Status |
|---|---|---|---|
| Automation | Record a macro | P2 | M |
| Automation | Pause and resume a recording | P2 | M |
| Automation | List and run macros | P2 | M |
| Automation | Macro shortcut key | P2 | M |
| Automation | Run a VBA project | P2 | M |
| Automation | Visual Basic editor | P2 | M |
| Automation | Keep a VBA project on save | P2 | D |
| Automation | Remove a VBA project from a drawing | P2 | P |

### Automation: Security

| Area | Feature | Priority | Status |
|---|---|---|---|
| Automation | Macro security settings | P2 | M |
| Automation | Trusted publishers and locations | P2 | M |
| Automation | Add-in trust settings | P2 | M |
| Automation | External app link settings | P2 | M |

### Automation: Add-ons and add-ins

| Area | Feature | Priority | Status |
|---|---|---|---|
| Automation | Run a bundled add-on | P2 | M |
| Automation | COM add-ins | P2 | M |
| Automation | Add-ins tab for older menus | P2 | M |
| Automation | Custom ribbon from a solution | P2 | M |

### Automation: Object model

| Area | Feature | Priority | Status |
|---|---|---|---|
| Automation | Program access to documents, pages and shapes | P2 | P |
| Automation | Run a command by id | P2 | D |
| Automation | Events a program can subscribe to | P2 | M |
| Automation | Events stored in a document | P2 | M |
| Automation | Data connections through the object model | P2 | M |
| Automation | Embed the drawing surface in another program | P2 | M |

### Automation: Developer tools

| Area | Feature | Priority | Status |
|---|---|---|---|
| Automation | Show the Developer tab | P2 | P |
| Automation | Controls on a drawing | P2 | M |
| Automation | Design mode for controls | P2 | M |
| Automation | Event Monitor tool | P2 | M |
| Automation | Persistent Events tool | P2 | M |
| Automation | Print ShapeSheet tool | P2 | M |
| Automation | Stencil window explorer | P2 | M |
| Automation | Drawing Page on the Stencil tab | P2 | M |
| Automation | Developer Reference | P2 | M |
| Automation | Master shortcut on a stencil | P2 | M |

### Review: Comments

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Add a comment to a shape or page | P1 | D |
| Review | Comment stays with its shape | P1 | D |
| Review | Comment markers on the page | P1 | D |
| Review | Show or hide comment markers | P2 | D |
| Review | Comments Pane | P1 | D |
| Review | Reply to a comment | P1 | D |
| Review | Edit a comment | P1 | D |
| Review | Delete a comment | P1 | D |
| Review | Step through comments | P2 | D |
| Review | Comments are not printed | P2 | D |

### Review: Track Markup

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Track Markup on and off | P2 | M |
| Review | Markup overlay per reviewer | P2 | M |
| Review | Overlay colour overrides formatting | P2 | M |
| Review | Reviewing pane | P2 | M |
| Review | Show or hide overlays | P2 | M |
| Review | Step through markup | P2 | M |
| Review | Delete markup | P2 | M |

### Review: Ink

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Draw with a pen on the canvas | P2 | M |
| Review | Group strokes into ink shapes | P2 | M |
| Review | Ink shapes behave as shapes | P2 | M |
| Review | Pen colour and thickness | P2 | M |
| Review | Stroke eraser | P2 | M |
| Review | Pressure-sensitive ink | P2 | M |
| Review | Highlighter pen | P2 | M |
| Review | Draw with touch | P2 | M |
| Review | Convert ink to geometry | P2 | M |
| Review | Convert ink to text | P2 | M |

### Review: Presenting

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Presentation mode | P1 | M |
| Review | Move between pages while presenting | P1 | M |
| Review | Page menu while presenting | P2 | M |
| Review | Legacy Mode | P2 | M |
| Review | Slide snippets for PowerPoint | P2 | M |
| Review | Export snippets to a slide deck | P2 | M |

### Review: Accessibility

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Accessibility Checker | P1 | M |
| Review | Alt text on shapes and pictures | P1 | M |
| Review | Alt text on masters | P2 | M |
| Review | Alt text on pages | P2 | M |
| Review | Diagram Navigation pane | P1 | M |
| Review | Screen reader reads shapes in order | P1 | M |
| Review | Move between pages with a reader | P2 | M |
| Review | High contrast support | P2 | M |
| Review | Tagged PDF output | P2 | M |

### Review: Spelling and language

| Area | Feature | Priority | Status |
|---|---|---|---|
| Review | Check spelling across a drawing | P1 | M |
| Review | Spelling actions | P1 | M |
| Review | Settings for the spelling check | P2 | M |
| Review | Check spelling as you type | P2 | M |
| Review | AutoCorrect | P2 | M |
| Review | Thesaurus | P2 | M |
| Review | Research pane | P2 | M |
| Review | Insights pane | P2 | M |
| Review | Translate text | P2 | M |
| Review | Language of shape text | P2 | M |
| Review | Language preferences | P2 | M |
| Review | Regional text options | P2 | M |
| Review | Horizontal in vertical text | P2 | M |
| Review | Reconvert East Asian text | P2 | M |

### Backstage: Open, save and close

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Open a drawing | P0 | D |
| Backstage | Recent drawings list | P1 | M |
| Backstage | Start screen | P1 | M |
| Backstage | Save a drawing | P0 | D |
| Backstage | Save As another name or location | P0 | D |
| Backstage | Save a Copy | P2 | M |
| Backstage | Save workspace with the file | P2 | M |
| Backstage | Default save format | P2 | M |
| Backstage | Close a drawing | P1 | M |
| Backstage | Exit the app | P1 | D |
| Backstage | Default working folders | P2 | M |
| Backstage | Warnings about bad XML in files | P2 | M |
| Backstage | Protected view and security bars | P2 | M |

### Backstage: Recovery

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | AutoRecover on an interval | P1 | M |
| Backstage | Keep the last version when closing unsaved | P1 | M |
| Backstage | Reopen after a crash | P1 | M |

### Backstage: Earlier versions

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Open and edit older binary and XML formats | P1 | M |
| Backstage | Compatibility Mode | P2 | M |
| Backstage | Convert to the current format | P2 | M |
| Backstage | Save in the older format | P2 | M |
| Backstage | Compatibility Checker | P2 | M |

### Backstage: File information

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | File properties | P1 | M |
| Backstage | Preview picture in the file | P2 | M |
| Backstage | Reduce file size | P2 | M |
| Backstage | Remove personal information | P2 | M |
| Backstage | Remove external data from a file | P2 | M |
| Backstage | Info view notices | P2 | M |
| Backstage | Fix-it hub on the Open view | P2 | M |

### Backstage: Export

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Export a page or selection to a graphic | P1 | P |
| Backstage | Export to PDF | P1 | P |
| Backstage | PDF page range and options | P1 | M |
| Backstage | Export to XPS | P2 | M |
| Backstage | Export to PNG | P1 | P |
| Backstage | Export to JPG | P1 | M |
| Backstage | Export to TIFF | P2 | M |
| Backstage | Export to GIF | P2 | M |
| Backstage | Export to BMP | P2 | M |
| Backstage | Export to EMF or WMF | P2 | M |
| Backstage | Export to SVG | P1 | P |
| Backstage | Include or leave out the drawing's data in SVG | P2 | M |
| Backstage | Raster export resolution and size | P1 | P |
| Backstage | Raster export rotation and flip | P2 | M |
| Backstage | Raster export settings last for the session | P2 | M |
| Backstage | Save as a web page | P2 | M |
| Backstage | Web page navigation controls | P2 | M |
| Backstage | Web page output format | P2 | M |

### Backstage: CAD drawings

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Open a DWG or DXF as a drawing | P2 | M |
| Backstage | Set a CAD drawing's scale on import | P2 | M |
| Backstage | Change a CAD drawing's scale | P2 | M |
| Backstage | CAD drawing locked on the page | P2 | M |
| Backstage | Crop a CAD drawing | P2 | M |
| Backstage | Show or hide CAD layers | P2 | M |
| Backstage | Change CAD layer colour and weight | P2 | M |
| Backstage | Convert a CAD drawing to shapes | P2 | M |
| Backstage | Convert many CAD drawings | P2 | M |
| Backstage | Save as DWG or DXF | P2 | M |

### Backstage: Other files and programs

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Open a picture as a new drawing | P2 | M |
| Backstage | Paste a drawing into another document | P2 | M |
| Backstage | Link a drawing into another document | P2 | M |
| Backstage | Edit links to other files | P2 | M |

### Backstage: Settings

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | User name and initials | P2 | M |
| Backstage | Enable Automation Events | P2 | M |
| Backstage | Default units for text, angle and duration | P2 | M |
| Backstage | Freeform tool precision and smoothing | P2 | M |
| Backstage | Delete connectors with their shape | P2 | P |
| Backstage | More shape handles on hover | P2 | M |
| Backstage | Animated transitions | P2 | M |
| Backstage | Center selection on zoom | P2 | M |
| Backstage | Enter key in shape text | P2 | P |
| Backstage | Stencil name wrapping | P2 | M |
| Backstage | Window background colours | P2 | M |
| Backstage | Office theme for the app | P2 | P |

### Backstage: Ribbon and status bar

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Ribbon with tabs and groups | P0 | D |
| Backstage | Contextual tabs | P1 | M |
| Backstage | Collapse the ribbon | P1 | D |
| Backstage | Key Tips on the ribbon | P1 | M |
| Backstage | Customize the ribbon | P2 | M |
| Backstage | Quick Access Toolbar | P1 | P |
| Backstage | Search for a command | P1 | M |
| Backstage | Status bar | P1 | D |
| Backstage | High DPI display support | P1 | D |
| Backstage | Touch mode for the ribbon | P2 | M |
| Backstage | Back and forward after a link | P2 | M |

### Backstage: Account and help

| Area | Feature | Priority | Status |
|---|---|---|---|
| Backstage | Sign in and switch accounts | P2 | M |
| Backstage | About the app | P2 | P |
| Backstage | Product updates | P2 | M |
| Backstage | Help pane | P1 | M |
| Backstage | Feedback command | P2 | M |
| Backstage | Training and what's new links | P2 | M |
| Backstage | Privacy settings | P2 | M |

### Cloud: Files in the cloud

| Area | Feature | Priority | Status |
|---|---|---|---|
| Cloud | Save a drawing to OneDrive or SharePoint | P2 | M |
| Cloud | Open a drawing from the cloud | P2 | M |
| Cloud | Add a cloud place | P2 | M |
| Cloud | Files others have shared | P2 | M |
| Cloud | View a prior version | P2 | M |
| Cloud | Restore a prior version | P2 | M |
| Cloud | Check a drawing out and in | P2 | M |
| Cloud | Server file states and cached copies | P2 | M |

### Cloud: Working with other people

| Area | Feature | Priority | Status |
|---|---|---|---|
| Cloud | Co-author a drawing | P2 | M |
| Cloud | See who else is in a drawing | P2 | M |
| Cloud | Merge changes on save | P2 | M |
| Cloud | Stop a document being co-authored | P2 | M |
| Cloud | Share with people | P2 | M |
| Cloud | Send a drawing by email | P2 | M |

### Cloud: Protection

| Area | Feature | Priority | Status |
|---|---|---|---|
| Cloud | Restrict access with Information Rights Management | P2 | M |

### Cloud: Other Microsoft services

| Area | Feature | Priority | Status |
|---|---|---|---|
| Cloud | A diagram in a Power BI report | P2 | M |
| Cloud | Diagrams in Microsoft Teams | P2 | M |
| Cloud | Embed a diagram in a web page | P2 | M |
| Cloud | A JavaScript API | P2 | M |
| Cloud | Move between the desktop and web apps | P2 | M |
| Cloud | Team document discovery through Delve | P2 | M |
| Cloud | Publish to a web viewer | P2 | M |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-25 | minor | A paragraph above the summary names the rows of the four features scheduled next after the 0.3 items, in the order the owner listed them (owner, 2026-11-25); no row's score changes |
| 2026-11-23 | trivial | Sentences reworded so that each reads whole on its own; nothing they say changes |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | The Organization Chart template and Re-layout an org chart score D, and Drop a shape on its manager, Org chart layout for a whole chart, Arrange one manager's subordinates and Org chart picture on a shape score P; weighted breadth 34% weighted (P0 99%, P1 45%, P2 6%) beside the swimlane and shape-search rows, Diagram kinds 11% |
| 2026-11-23 | minor | Search for shapes and Quick Shapes move from M to D and Shape search options from M to P (the summary is recomputed by `cargo xtask parity`) |
| 2026-11-23 | minor | Cross-Functional Flowchart template, Insert a swimlane, Swimlane holds its shapes, Swimlane label text and direction and Phase separators move from M to D, and Swimlane orientation and Move and resize swimlanes from M to P (the headline is 33%: P1 43%, Diagram kinds 8%) |
| 2026-11-22 | minor | Apply a theme (a live preview of a theme, a variant and a colour variant while its tile is under the pointer, on a copy of the drawing: no undo step, no unsaved mark, the redo kept), Fill colour (the palette's Use Default, Theme Colors, Variant Colors, Standard Colors, Recent Colors, No Fill and More Colors, and the Colors dialog), Custom colours by RGB or hex and Standard colours stay put move from P to D, each with its evidence (`theme.colors`, `theme.preview`, a theme colour, `default` in `shape.format` and `text.format`, `window.colorsDialog`); the headline is recomputed by `cargo xtask parity` |
| 2026-11-22 | minor | Auto Size extends the page, Snap on or off, Zoom controls and keys, Open a stencil from More Shapes, Group and ungroup, Horizontal paragraph alignment, Right-angle connectors, Edit a connector path by hand, Arrowheads on lines, Start a drawing from a template, Template opens its stencils, Basic Flowchart template, Basic Diagram template and Ribbon with tabs and groups move from P to D, and Automatic reroute on moving shapes, Apply a theme and Fill colour stay P with what each would take; P0 done 39 to 53 and partial 17 to 3, P0 85% to 97% and the weighted total 30% to 32% (P1 40%, P2 6%; measured: `cargo xtask parity`) |
| 2026-11-22 | minor | Page breaks shown on the page, Move off Page Breaks and Avoid Page Breaks option move from M to D, and Re-Layout Page, Auto Space and Spacing Options and Layout placement, flip and spacing options from M to P (`page.relayout`, `shape.autospace`, `page.spacing`, `page.breaks`, `page.avoid_page_breaks`, `shape.move_off_page_breaks`) |
| 2026-11-22 | minor | Apply a Quick Style, Fill transparency, Gradient fill, Pattern fill, Line transparency, Drop shadow and Format Shape pane with live preview move to D, and so do Gradient rotates with the shape, Line cap, Compound line, Gradient line, Format Painter and Format Painter on many shapes; Glow and Soft edges stay M |
| 2026-11-22 | minor | Shape Data window, Enter shape data values, Add and delete shape data fields, Shape data field types, Fixed and variable list fields, Field display format, Prompt text on a field, Hidden shape data fields and Shape data field order move from M to D and Ask for data on drop and Field language and calendar from M to P; P1 done 70 to 75 and missing 173 to 168, P2 done 12 to 16, partial 27 to 29 and missing 586 to 580, Data 0% to 14%, and the weighted total 26% to 27% (P0 85%, P1 34%, P2 5%; measured: `cargo xtask parity`, beside the container row) |
| 2026-11-22 | minor | Layer lock moves from P to D; `cargo xtask parity` recomputes the summary |
| 2026-11-22 | minor | Eleven rows of the containers score D or P, so the headline breadth is 26% (P0 85%, P1 32%, P2 4%) and the Shapes area is 58% |
| 2026-11-22 | minor | Layer snap and glue moves from P to D and Layer lock stays P; P2 done 9 to 10 and partial 25 to 24, and the weighted total stays 25% (P0 85%, P1 30%, P2 4%; measured: `cargo xtask parity`, beside the Change Shape row) |
| 2026-11-22 | minor | Callout follows its shape moves from M to D and Add a callout from M to P (`callout.insert`, `callout.attach`, `callout.detach`; the Insert, Diagram Parts, Callout gallery and the yellow handle) |
| 2026-11-22 | minor | Change Shape moves from M to D (`shape.change` and the Home, Editing, Change Shape gallery); P1 done 61 to 62 and missing 182 to 181, Shapes 51% to 52%, the weighted total from 24% to 25% (P0 85%, P1 30%, P2 3%; measured: `cargo xtask parity`) |
| 2026-11-22 | minor | Hyperlinks on a shape and Follow a hyperlink move from M to D (`hyperlink.set`, `hyperlink.follow`; the Hyperlinks dialog, Cmd+K, and a Ctrl+click), Insert symbol from M to D (the Symbol menu and dialog, `text.insert`) and Insert a picture from file from M to P (`insert.picture`, Insert, Illustrations, Pictures: a GIF or a TIFF is held and drawn as a placeholder, an SVG is refused); Text fields stays P, since a field is inserted by a dialog and not in the text box; P1 done 59 to 61, partial 52 to 53 and missing 185 to 182, P2 done 8 to 9 and missing 592 to 591, Shapes 49% to 51%, P1 29% to 30%, and the weighted total stays 24% (P0 85%, P1 30%, P2 3%) |
| 2026-11-22 | minor | Create, rename and delete layers, Assign shapes to layers, Active layers, Layer visibility, Layer printing, Layer colour and transparency and Remove unreferenced layers move to D, Layer lock and Layer snap and glue to P; `cargo xtask parity` recomputes the summary |
| 2026-11-22 | minor | Lasso selection, Select partly enclosed shapes and Select by Type move from M to D (Home, Editing, Select: Select by Type, Area Select and Lasso Select work, and a net or a loop takes the shapes wholly inside it or, with the Advanced option, those it touches); P1 done 52 to 54 and missing 192 to 190, P2 done 5 to 6, Shapes 47% to 49%, P1 26% to 27%, the weighted total stays 23% (P0 85%, P1 27%, P2 3%; measured: `cargo xtask parity`) |
| 2026-11-22 | minor | Case changes, Bulleted lists and Right-to-left text direction move from M to D, and Indents and paragraph spacing and Vertical text box from M to P, P1 done from 50 to 52 and partial from 51 to 52, P2 done from 4 to 5 and partial from 23 to 24, Shapes from 45% to 47%, and the weighted total from 22% to 23% (P0 85%, P1 26%, P2 3%) |
| 2026-11-22 | minor | Find text and Replace text move from M to P (`edit.find`, `edit.replace`, and the Find and Replace dialogs of Home, Editing, Find, which are not modals); P1 partial 49 to 51, missing 197 to 195, P1 25% to 26%, and the weighted total stays 22% |
| 2026-11-22 | minor | Rectangle and ellipse tools, Line tool, Build a shape from segments, Arc tool, Freeform tool and Close a path into a filled shape move from M to D, Pencil tool from M to P (the vertex editing is Edit vertices, still M); P1 done 44 to 50, partial 48 to 49, Shapes 40% to 45%, weighted total 21% to 22% (measured: `cargo xtask parity`) |
| 2026-11-22 | minor | About the app moves from M to P (`app.about`: the File view's About, and the splash at the start); P2 partial 21 to 22, Backstage 21% to 22%, the weighted total stays 21% (measured: `cargo xtask parity`) |
| 2026-11-21 | minor | Duplicate a page, Create a background page, Turn a foreground page into a background and Assign a background to a page move from M to D (`page.duplicate`, `page.add {background}`, `page.type`, `page.background` and the window's Insert, page tab and Page Properties controls); the summary is recomputed |
| 2026-11-21 | minor | Cut, copy and paste, Duplicate by Ctrl-drag, Nested groups and Show or hide guides move from P to D and Automatic reroute on moving shapes from D to P, after a change read the rows again; Duplicate command, Glue shapes to guides, Choice of snap targets, Dynamic grid and Measurement units per page keep their scores (scores err low); the weighted headline stays 20% (19.96% to 20.18%), P1 goes from 35 done and 52 partial to 38 and 49, Document from 44% to 45%, Shapes from 39% to 40% and Connectors from 41% to 40% |
| 2026-11-21 | minor | The scoring note on the clipboard, group and stencil controls says they were wired in and that their rows keep the scores they had until the rows are read again; no score changed |
| 2026-11-19 | minor | No score moves; the engine takes `lock`, `nudge`, `view`, `keep` and `radius` and tells `marks`, and the window does not send them yet |
| 2026-11-13 | minor | Accepted compact stencil chooser, square icon cards and name-only tooltips; parity scores unchanged |
| 2026-11-12 | minor | Decorative Shapes, Arrow Shapes and basic Gane-Sarson add 20 masters; five stencils and 50 masters; new masters are refused on ODG save |
| 2026-11-11 | minor | Accepted interface typography, neutral surfaces, quieter grid and clearer controls; retained; parity scores unchanged |
| 2026-11-11 | minor | Application icon belongs to the OS window decoration; removed the duplicate in-app mark |
| 2026-11-11 | minor | Approved icon in the desktop and web app, cover in File Home and README; parity scores unchanged |
| 2026-11-11 | minor | Option A gives stencil previews thin graphite outlines, smooth curves and borderless idle tiles; parity scores unchanged |
| 2026-11-08 | minor | Glue master switch and targets moves from M to P |
| 2026-11-08 | minor | Show or hide guides moves from M to P |
| 2026-11-08 | minor | Glue shapes to guides moves from M to P (engine only) |
| 2026-11-07 | minor | Create and delete guides, Choice of snap targets and Dynamic grid move from M to P (engine only); Snap on or off's evidence names `snap.set` |
| 2026-11-07 | minor | VBA out of scope by policy: Keep a VBA project on save is D (every save keeps it), Remove a VBA project from a drawing is P (engine only) |
| 2026-11-07 | minor | Size & Position window, Units and arithmetic in Size & Position and Rotate by typed angle move to D; Pin position, Pan & Zoom window, Drawing Explorer window and Format Shape pane with live preview to P; the headline is 19% |
| 2026-11-07 | minor | Three template rows move from M to P (engine only); P1 from 16% to 17% |
| 2026-11-07 | minor | Six theme rows move from M to P (engine only), and the Styling area from 18% to 27% |
| 2026-11-07 | major | The 43 provisional rows are replaced by the 977 rows of the feature inventories, each scored against the commands and the window as they are |
| 2026-10-31 | major | Created for M0 with provisional rows |
