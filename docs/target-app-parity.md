# Target-app parity: DiaCraft vs the reference app

> **Last reviewed:** 2026-11-07

The authoritative assessment of how close DiaCraft is to its target. The target is the reference app's desktop edition. Today a small diagram can be drawn, formatted, saved and opened again, and most of the feature list is open: [`parity-checklist.md`](parity-checklist.md) scores every row of the feature inventories against the code, and [`parity-evidence.md`](parity-evidence.md) says what each done or partial row rests on.

## Headline

| Number | Value | Kind |
|---|---|---|
| Feature breadth | 34% weighted (P0 99%, P1 45%, P2 6%) over 977 rows | measured (`cargo xtask parity` over the inventory rows, 2026-11-12); each D/P/M score remains an estimate supported by its recorded evidence |
| Ready for real work | 0% | estimated (a small diagram can be drawn and saved, but no real drawing has been checked against the reference app) |

## Target and how it was measured

The target is the reference app's desktop edition. The reference app was not inspected, nothing was launched or measured, and our output has not been opened in it by the project. Target features and the menu tree come from public documentation only, cited by URL and date.

Evidence, strongest first: public sample files as an oracle, Microsoft's published schema, independent readers such as LibreOffice, round-trip stability, and public reports. The inventory's evidence lists in `/` record which each row rests on, and grading is by behaviour, not by whether a menu item exists. Claims about the real reference app stay marked unverified.

## Areas

| Area | State | Evidence |
|---|---|---|
| Engine and commands | Registry, undo, panic guard and batches over the ShapeSheet model, with the editing, format, arrange, page setup, snapping, theme, shape operation, layer, shape data, callout, container, comment, swimlane, layout, file and export commands | Tests and command sweep |
| Interface | The window is laid out as the spec has it: title bar, ribbon, Shapes window, drawing area with rulers and grid, page tabs, status bar, File view; zoom, pan, pages and the View tab's switches work, a file opens (a dialog, a path argument) and every page of it is drawn, shapes of the built-in stencils are dropped, selected, moved, resized, joined by connectors and labelled with the mouse and keyboard, and a drawing is saved (a dialog, Ctrl+S) and asked about before it is thrown away; the Home tab fills and lines shapes, sets fonts, sizes, styles, colours and alignment of their text (or of characters selected in the text box), and aligns, turns, flips and reorders them, with the keys, a rotation handle and the handles of a turned shape, a Format Painter, a Zoom dialog and a Snap box in the View tab, and the Design tab's Connectors menu and Themes and Variants galleries; gives vector stencil previews thin unfilled graphite outlines and borderless idle tiles in both themes; uses the approved octopus as the OS window icon, with no duplicate mark in the in-app top bar, and the matching cover remains in File Home; adds optional regular interface fonts, neutral surfaces, quieter grid lines and consistent hover and focus feedback; adds a compact stencil chooser, current-stencil filter and uniform square icon cards with name-only hover tooltips; supplies original decorative and arrow shapes and basic Gane-Sarson nodes and a Data Flow connector, and the Cross-Functional Flowchart Shapes and the Organization Chart Shapes, for seven built-in stencils and 58 masters; many other controls are drawn and disabled | Screenshots of the real window were read; `docs/ui-parity.md` |
| Agents | CLI and MCP on the same registry | Real client run |
| Everything else | Missing, as the checklist scores it: the Design tab's theme Effects and Embellishments (the Themes and Variants galleries work, with a live preview of a theme), Shape Data sets and most rows of the later areas (duplicating pages, layers, containers, comments and swimlanes are built) | [`parity-checklist.md`](parity-checklist.md) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-23 | trivial | Sentences reworded so that each reads whole on its own; nothing they say changes |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | The Interface row says seven built-in stencils and 58 masters |
| 2026-11-23 | minor | The headline breadth is 34% weighted (P0 99%, P1 45%, P2 6%) beside the swimlane and shape-search rows with the Organization Chart template and Re-layout an org chart done and Drop a shape on its manager, Org chart layout for a whole chart, Arrange one manager's subordinates and Org chart picture on a shape partial; the catalogue has seven stencils and 58 masters (the Organization Chart Shapes), four templates, and the registry has 178 commands, measured with `diacraft-cli commands` |
| 2026-11-23 | minor | The headline breadth is 33% weighted (P0 99%, P1 43%, P2 6%, recounted with the swimlane rows) with Search for shapes and Quick Shapes done (two P1 rows, M to D) and Shape search options partial (one P2 row), and the Shapes area is 62% |
| 2026-11-23 | minor | The Engine and commands row names the layer, shape data, callout, container, comment, swimlane and layout commands, the Interface row says many other controls are disabled (it said most), and Everything else no longer lists duplicating pages, layers and a theme's live preview as missing |
| 2026-11-23 | minor | The headline breadth is 33% (P0 99%, P1 43%, P2 6%) with the Cross-Functional Flowchart template, Insert a swimlane, Swimlane holds its shapes, Swimlane label text and direction and Phase separators done and Swimlane orientation and Move and resize swimlanes partial; the catalogue has six stencils and 52 masters (the Cross-Functional Flowchart Shapes), three templates, and the registry has 165 commands, measured with `diacraft-cli commands` (it said 146, and counted before the commands of several earlier tasks) |
| 2026-11-22 | minor | The headline breadth stays 32% weighted (P0 99%, P1 41%, P2 6%): Apply a theme and Fill colour are done, with Custom colours by RGB or hex and Standard colours stay put (the palette's Theme, Variant, Standard and Recent Colors, the Colors dialog, and a live preview of a theme under the pointer); one P0 row, Automatic reroute on moving shapes, is partial |
| 2026-11-22 | minor | The headline breadth is 32% (P0 97%, P1 40%, P2 6%) with Auto Size extends the page, Snap on or off, Zoom controls and keys, Open a stencil from More Shapes, Group and ungroup, Horizontal paragraph alignment, Right-angle connectors, Edit a connector path by hand, Arrowheads on lines, Start a drawing from a template, Template opens its stencils, Basic Flowchart template, Basic Diagram template and Ribbon with tabs and groups done, and Automatic reroute on moving shapes, Apply a theme and Fill colour still partial |
| 2026-11-22 | minor | The headline breadth stays 30% (P0 85%, P1 40%, P2 6%) with the page breaks, Move off Page Breaks and Avoid Page Breaks rows done and Re-Layout Page, Auto Space and Spacing Options and the layout options rows partial |
| 2026-11-22 | minor | The headline breadth is 30% (P0 85%, P1 40%, P2 6%) with the Review, Comments rows done (`comment.add`, `comment.reply`, `comment.edit`, `comment.delete`, `comment.list`, `comment.markers`, `comment.next`, `comment.previous`, `comment.author`) |
| 2026-11-22 | minor | The headline breadth is 29% (P0 85%, P1 37%, P2 6%) with the Styling area at 57% (Quick Styles, transparency, gradient and pattern fill, drop shadow, the Format Shape pane and the Format Painter), and four engine commands more |
| 2026-11-22 | minor | The headline breadth is 27% (P0 85%, P1 34%, P2 5%) with the five P1 rows of the Shape Data window done (Shape Data window, Enter shape data values, Add and delete shape data fields, Shape data field types, Fixed and variable list fields) and Field display format, Prompt text on a field, Hidden shape data fields and Shape data field order done and Ask for data on drop and Field language and calendar partial (the Shape Data pane, the Define Shape Data dialog and the card a drop asks) |
| 2026-11-22 | minor | The headline breadth stays 26% (P0 85%, P1 33%, P2 4%) with Layer lock done |
| 2026-11-22 | minor | The headline breadth is 26% (P0 85%, P1 32%, P2 4%) with the Add a container, Container membership by drop, Members move, copy and delete with the container, Container style and margins, Container fit and auto resize, Lock a container, Disband a container and Select container contents rows done and Attach a shape to a container edge, Connect to a container and Container heading position partial, and the engine has thirteen commands more (the `container.*` commands) |
| 2026-11-22 | minor | The headline breadth is 25% (P0 85%, P1 30%, P2 4%) with the Layer snap and glue row done (a connector is glued anew to no shape whose layers allow no glue, and AutoConnect offers none); Layer lock stays partial, every command that changes the drawing now refusing a locked shape it names |
| 2026-11-22 | minor | The headline breadth is 25% (P0 85%, P1 30%, P2 3%) with the Callout follows its shape row done and Add a callout partial, and the engine has five commands more: `callout.styles`, `callout.insert`, `callout.attach`, `callout.detach` and `callout.list` |
| 2026-11-22 | minor | The headline breadth is 25% (P0 85%, P1 30%, P2 3%) with the Change Shape row done (Home, Editing, Change Shape: `shape.change` and a gallery of the open stencils' masters that fit the selection) |
| 2026-11-22 | minor | The headline breadth stays 24% (P0 85%, P1 30%, P2 3%) with the Hyperlinks, Follow a hyperlink and Insert symbol rows done and Insert a picture from file partial, and the engine has nine commands more: `insert.picture`, `hyperlink.set`, `hyperlink.list`, `hyperlink.follow`, `text.insert_field`, `text.field_choices`, `text.field_sample`, `text.update_fields` and `text.insert` |
| 2026-11-22 | minor | The headline breadth is 24% (P0 85%, P1 29%, P2 3%) beside TEXTFMT's and SELECT's rows with Create, rename and delete layers, Assign shapes to layers, Active layers, Layer visibility, Layer printing, Layer colour and transparency and Remove unreferenced layers done and Layer lock and Layer snap and glue partial |
| 2026-11-22 | minor | The headline breadth is 23% (P0 85%, P1 27%, P2 3%) with the Lasso selection, Select partly enclosed shapes and Select by Type rows done |
| 2026-11-22 | minor | The headline breadth is 23% (P0 85%, P1 26%, P2 3%) with the Case changes, Bulleted lists and Right-to-left text direction rows done and the Indents and paragraph spacing and Vertical text box rows partly |
| 2026-11-22 | minor | The headline breadth is 22% (P0 85%, P1 26%, P2 2%) with Find text and Replace text partial |
| 2026-11-22 | minor | The headline breadth is 22% (P0 85%, P1 25%, P2 2%) with the drawing tools rows (Rectangle and ellipse, Line, Arc, Freeform, Build a shape from segments, Close a path) done and the Pencil partly |
| 2026-11-22 | minor | The headline breadth is 21% (P0 85%, P1 23%, P2 2%) with AutoConnect arrows and flyout, AutoConnect drag to an existing shape and Drop a master on an AutoConnect arrow done |
| 2026-11-21 | minor | The headline breadth is 21% (P0 84%, P1 22%, P2 2%) with the page copy and background rows done |
| 2026-11-21 | minor | The Everything else row no longer lists the Design tab's theme galleries as missing (the Themes and Variants galleries work); the Interface row adds the Format Painter, the Zoom dialog, the View tab's Snap box and the Design tab's Connectors menu and galleries; parity scores unchanged |
| 2026-11-21 | minor | The Engine and commands area says the registry works over the ShapeSheet model (it said a stub document) and names the families of commands, and the Interface area says shapes of the built-in stencils are dropped (it said Basic Shapes); parity scores unchanged |
| 2026-11-19 | minor | The engine adopts `craft-snap`; the window part is a task of its own, so the snapping rows keep their scores |
| 2026-11-13 | minor | Accepted compact stencil chooser, square icon cards and name-only tooltips; parity scores unchanged |
| 2026-11-12 | minor | Decorative Shapes, Arrow Shapes and basic Gane-Sarson add 20 masters; five stencils and 50 masters; new masters are refused on ODG save |
| 2026-11-11 | minor | Accepted interface typography, neutral surfaces, quieter grid and clearer controls; retained; parity scores unchanged |
| 2026-11-11 | minor | Application icon belongs to the OS window decoration; removed the duplicate in-app mark |
| 2026-11-11 | minor | Approved icon in the desktop and web app, cover in File Home and README; parity scores unchanged |
| 2026-11-11 | minor | Option A gives stencil previews thin graphite outlines, smooth curves and borderless idle tiles; parity scores unchanged |
| 2026-11-07 | minor | Themes in the engine: six theme rows are P, and the headline is 18% (P0 84%, P1 16%) |
| 2026-11-07 | minor | The headline breadth is 16% with the connector commands in the engine |
| 2026-11-07 | minor | The headline breadth is 17% with page setup, the page tabs and the text tools in the window |
| 2026-11-07 | minor | The headline breadth is counted over the 977 inventory rows and the Everything else row points at the checklist |
| 2026-11-07 | minor | The Interface row follows a window that formats and arranges the selection |
| 2026-11-01 | minor | The Interface row follows a window that draws, joins and saves |
| 2026-11-01 | minor | The Interface row follows the window that opens and draws files |
| 2026-11-01 | minor | The Interface row follows the drawn window |
| 2026-10-31 | major | Created for M0 |
