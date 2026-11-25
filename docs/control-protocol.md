# Control protocol

Started with `--control <port>`, DiaCraft accepts TCP connections at the address `127.0.0.1` (the loopback, so only
this machine) on that port. A client can then drive the running window: run commands, look at the document and the
window, and take a screenshot. Tests, agents, the CLI and the MCP
server all use it. It is the same registry of commands the menus use: `engine.execute` is
`Session::execute`, so a command behaves the same however it was asked for.

The port is a number from 1 to 65535, given once, as `--control 7997` or `--control=7997`; without the flag the
`DIACRAFT_CONTROL_PORT` variable names it. A bad value stops the app before a window opens. The examples here use
7997. The sibling craft apps default to 7979, so a port that is already in use is not unusual: see
[A busy port](#a-busy-port).

**The port has no password.** Any program on the machine can connect and run any command, and `ui.screenshot`
writes a PNG to any folder the user can write to (replacing a PNG already there). Start the app with `--control` only while you drive it. Web pages
cannot use the port: an HTTP request's first line is not JSON, so the connection is closed before its body is read
(see below).

## Requests and answers

One JSON object per line, in and out, ended by `\n` (`\r\n` also works; blank lines are skipped):

```
{"id": 7, "method": "ui.inspect", "params": {}}
{"id":7,"ok":true,"result":{"app":"diacraft","version":"0.2.0","frames":4,...}}
{"id":8,"ok":false,"error":"unknown method `engine.explode`; the methods are: engine.execute, ..."}
```

- `method` is a string and is required. `params` is an object, or left out, or `null` (all the same as `{}`).
- `id` is optional: a number, a string or `null`. The answer carries it back, `null` when there was none.
- The answer is `{"id", "ok": true, "result"}` or `{"id", "ok": false, "error"}`, one line each, in the order the
  requests were sent. A client may send a request, wait for its answer, and send the next on the same connection.
- Every method takes a fixed set of keys. A key it does not know is an error, never ignored: a client that
  misspells `params` hears about it. The engine does the same for a command's own parameters.

### What closes the connection

The server only ever reads requests. Anything else gets **one** error answer (`{"ok": false, "error": "… closing the
connection"}`), after which that connection is closed and whatever the client queued behind the bad line is discarded and
never runs:

- text that is not JSON (an HTTP request line, a stray `nc`, a byte order mark, trailing text after the object);
- JSON that is not an object (`[1,2]`, `42`, `null`) or an object with no string `method`;
- bytes that are not UTF-8;
- a line longer than 4 MiB;
- a value nested more than 128 levels deep (`bad JSON (recursion limit exceeded …)`). This is `serde_json`'s own
  limit, kept on purpose: a request cannot make the server recurse deeper;
- a line that does not arrive in time (below).

A request object with a key other than `id`, `method` and `params`, or an `id` that is not a number, a string or
`null`, is a request that is wrong, not junk. It gets an error answer addressed to it (`id` echoed when it was
valid) and the connection stays open.

A client that gets an error answer to junk should reconnect.

## Limits

| What | Limit | When it is hit |
|---|---|---|
| Connections served at once | 16 | The 17th gets `{"ok": false, "error": "too many control connections …"}` and is closed. A place frees as soon as a connection closes. |
| Length of a request line | 4 MiB (4,194,304 bytes, without the newline) | One error answer, then close. |
| Idle between requests | 5 minutes | The connection closes **without** a line: an error line would be read as the answer to the client's next request. |
| A line, from its first byte to its newline | 30 seconds, absolute (a client that sends a byte at a time gains no time) | The error `request line not completed in time`, then close. |
| Writing an answer | 60 seconds | The connection closes (a peer that does not read cannot hold the writer). |
| The window answering a request | 60 seconds | A timeout error, below. |
| Requests the window answers in one frame | 64 | Later ones wait in the queue and another frame is asked for. |
| A screenshot waiting for a frame | 8 seconds | An error, below. Not more than one at a time. |
| Size of a screenshot | 128 MiB of pixels (a 4K window at 2x) | An error before anything is captured. |

Each request gets fresh line and write budgets; the 60 seconds for the window to answer is separate from them.

### A request the window did not answer

After 60 seconds the answer is `{"ok": false, "error": "timeout: …"}`, and it says which of two things happened:

- `timeout: the window did not start the request in time; it was withdrawn and will not run`. The window was busy or
  its frames were not being drawn. The request is withdrawn from the queue and never runs, so **retrying is safe**.
- `timeout: the window is still running the request; it may yet complete, so check before trying again`. The
  window had taken it. Look at the document (`document.inspect`) before retrying, or the work may be done twice.

If the window closes before it answers, the error says so (`the window closed before it answered`); that is not a
timeout.

### A busy port

If something else already listens on the port (another craft app, say), the app does not exit and does not
panic. It logs `control port 7997 is already in use: …`, shows a notice in its status bar (`The control channel is
off: port 7997 is already in use. …`), and runs without the channel. Start it again with another port. **On macOS** a
program listening on every address (`0.0.0.0`) does not hold the port against the app: macOS lets the app's loopback
listener share the port, and the loopback's connections then reach DiaCraft (measured on macOS 26.7). A program
on the loopback itself holds it there too.

### A hidden window

A window that is minimised, covered, on another desktop or behind a locked screen runs no frames. The control
channel still answers (the window keeps running its logic, a few times a second: about 0.25 s per request was
measured behind a locked screen), and `frames` in `ui.inspect` stops counting. `ui.screenshot` asks for a frame
anyway, but a locked screen presents none, and then the answer is the error described below.

## Methods

### `engine.execute {command, params?}`

Runs the command `command` (an id such as `file.new` or `page.add`, from `engine.commands`) with `params` (an
object; the command's own parameter doc says what it takes). The answer's `result` is whatever the command
returns; an error is the engine's: an unknown command, a command that cannot run now (`no document open`), bad
parameters (with the nearest key it knows), or a failure. A failed command changes nothing.

**`file.open` is the one command answered later** (and `file.new` with a `template` that is the path of a file: the same thread, the same progress card, the same answer, the new drawing's own result with `template`, `stencils` and `file`; a template that is an id is made at once). The window reads the file on a thread of its own, so that a large file never
holds the window, and answers when the reading ends: with the command's result (the document, and `file` with the reader's counts
and first warnings, and `stencil_notes`, a list of sentences, when a stencil the window had open was not opened again in the new drawing), or with its error. The answer is an error at once if another file is being opened (`Still opening <name>: wait
for it to finish.`). A file that cannot be opened leaves the open drawing as it was. **A file opened while a cancelled read is still parsing waits for it** (a read holds the memory a read may use while it runs; the wait is up to 30 seconds) and is then read against what that read left: it is refused in words as `out_of_memory` only where the two do not fit together or the wait ends. The window itself shows a progress card while
the file is read, and, for a file that could not be opened, a message in plain words that is dismissed with a button.

```
{"id":1,"method":"engine.execute","params":{"command":"file.new","params":{}}}
{"id":1,"ok":true,"result":{"history":{"redo":0,"undo":0},"pages":[{"id":1,"name":"Page-1"}],"title":"Untitled","version":1}}
{"id":2,"method":"engine.execute","params":{"command":"page.add","params":{"name":"Second"}}}
{"id":2,"ok":true,"result":{"id":2,"name":"Second"}}
```

### `engine.commands`

Every command, with where it sits in the menus, its shortcut, its parameter doc, whether it can run now and, if
not, why (`disabled_reason`).

```
{"id":3,"method":"engine.commands"}
{"id":3,"ok":true,"result":[{"id":"app.version","label":"Version","menu":[],"params":"none","shortcut":null,"enabled":true}, …]}
```

### `document.inspect`

The command of that name: the open document, `{title, version, pages: [{id, name}], history: {undo, redo}}`. An
error (`no document open`) when there is none.

### `ui.inspect`

The state of the window:

```
{"app":"diacraft","version":"0.2.0","frames":4,"window":[1280.0,720.0],"revision":1,
 "status":"Untitled · 1 page","document":{"title":"Untitled","pages":1},"notice":null,
 "panics":{"engine":0,"ui":0},"screenshotPending":false,"quitRequested":false}
```

`ui` is the window's own state (the output of the window's commands), among it `tool` (`pointer`, `connector`, `text`, `text_block`, `rectangle`, `ellipse`, `line`, `freeform`, `arc` or `pencil`), `pageSetup` (the tab the Page Setup dialog is open on, or null), `reorderPages` (the Reorder Pages dialog is open) and `panes` (the panes open, a list of the panes of each space from the top of the stack down), `explorerOpen` (the branches of Drawing Explorer that are unfolded), `textDialog` (the tab the Text dialog is open on, or null), `snapGlue` (the tab the Snap & Glue dialog is open on, or null), `options` (the tab the Options dialog is open on, or null) and `textBlock` (with the Text Block tool on and one shape selected: `{corners, pin, direction}` of the shape's text block on the page in inches, the four corners in the order of the block's own frame, the pin it turns about and the way the text runs in degrees; else null). `frames` counts the frames drawn. `window` is the size in points. `revision` moves whenever the document does.
`status` is what the status bar says; `notice` is the text of a notice in it, already escaped, or null. A notice goes eight seconds after the window first shows it (by the window's clock) or when the next command that changes something succeeds, whichever is first; a query such as `document.inspect` does not take it away, and a print's or an export's notice stays until the work ends. `opening` is `{file, seconds}` while a
file is being read, else null; `openError` is the message about the last file that could not be opened, until it is dismissed. `saveNote` is the save note on the screen: the list of its parts, in the engine's words for the last save's `carried` field and then for its `checked` field (what the file read back differs in), or null; it is not a warning, and a program that saves with `engine.execute` gets `carried` and `checked` in the result and no card.
`picture` says how many pictures of the page the render worker was asked for and gave (`requested`, `received`), whether one is
awaited (`busy`), how long the last took (`tookMs`), and the one on the screen (`shown`: its size, `litPixels`, the pixels that are
not clear, and `rough`, drawn within the short limits of a view that moved). `findings` is the list behind the status bar's small mark:
`{count, lines}`, in plain words. `panics` counts the
panics the guards caught, in the engine and in the window (each is a bug; the window carried on). A panic in drawing is the window's
own: `ui` counts a frame the render worker or the renderer's own net lost, and each shape the renderer lost to a bug, so a page that
drew blank never reads as 0. `picture.caughtPanics` splits that count into `framesLost` and `shapesLost`, and `picture.lost` is the
sentence the page shows over itself when its drawing failed and nothing is on it (null otherwise); the element `canvas.lost` is that
sentence on the page.

### `ui.screenshot {path}`

Writes the next frame the window presents to `path` as an 8-bit RGBA PNG, and answers
`{"path": "<path>", "width": 1472, "height": 828}` (pixels, so the window's size in points times its scale). `path`
must be absolute, end in `.png`, have no control characters, and be in a folder that exists. If something is already
at `path` it must be a regular file: a folder, a named pipe or a link is refused before anything is captured. The
file is written beside `path` and renamed over it, so it is replaced whole or not at all (a full disk leaves the old
file). Take the screenshot after the commands whose effect you want to see: the frame is the one painted after the
requests before it.

If no frame is presented within 8 seconds (a locked screen, a window nobody can see) the answer is
`{"ok": false, "error": "no frame was presented in time, so there is no screenshot (the screen may be locked, or the window minimized or covered)"}`
and no file is written. A frame that arrives later is dropped. A second request while one is waiting is refused
(`a screenshot is already being taken`).

### `ui.elements {prefix?}`

The controls the last frame drew, by name: `[{"name": "tab.View", "x": 530, "y": 52, "w": 44, "h": 26, "enabled": true}, …]`
(`x`, `y`, `w` and `h` in points, from the window's top left). `enabled` says whether the control can be used: a control
that does nothing yet is listed and is not enabled. `prefix` keeps the names that start with it. Names are
`tab.<label>` (and `tab.overflow` with `tab.overflow.<label>` in a narrow row), `tab_end.Comments`, `tab_end.Share`,
`ribbon.<tab>.<control id>` (the id the ribbon map gives), `ribbon.<tab>.group.<label>`, `ribbon.<tab>.fold.<group id>`
(a group folded to one button), `menu.<control id>` (an item of an open menu), `qat.Save`, `qat.Undo`, `qat.Redo`,
`titlebar.search`, `page-tab.<page id>` (the pages in the order of the tabs: foreground, then background), `page-tabs.all`, `page-tabs.add`, `shapes.<name>`,
`status.<name>` (`status.findings` is the small mark for what could not be drawn), `canvas`, `canvas.ruler.top` and `canvas.ruler.left` (the rulers, while they show: a drag out of one makes a guide), `canvas.guide.<id>` (a band along a guide's line in the drawing area, while guides show; its middle is the middle of the area, so a drag that starts there takes a shape under it first), `backstage.<name>`, `open-error.dismiss` (the button of the
message about a file that could not be opened), `save-note` and `save-note.dismiss` (the card of the save note and its button), `macros.save_without`, `macros.save_as_enabled` and `macros.cancel` (the buttons of the card "Leave the macros out?") and `opening.cancel` (the Cancel button of the card shown while a file is read). While one right-angle connector is selected with the Pointer Tool, `connector.leg.<n>` is the press area of the handle on its leg `n` (counted from the begin), and `legs` in the window state tells them (`{connector, handles: [{leg, level, at}], reroute, dragging: {leg, to, raw, snap}}`, `null` with none). While AutoConnect's arrows show (the pointer has rested on a shape with the Pointer Tool), `autoconnect.arrow.right`, `.left`, `.up` and `.down` are their press areas (a drag from one onto a shape joins the two; there is no circle handle, the elements `autoconnect.knob.<side>` of having gone), and while its flyout is open `autoconnect.flyout` is its card and `autoconnect.flyout.<n>` its tiles, in the order of the masters. In the Shapes window `shapes.stencil.<name>` is the row of a stencil, `shapes.more.<name>` its entry in the More Shapes flyout (while it is open) and `shapes.tile.<name>` a master's tile. The list is from the last frame, so a request sent in the same batch as the click that makes a
control appear does not see it yet: wait a frame.

The stencil gallery keeps `shapes.more` for the compact chooser and `shapes.more.<name>`, `shapes.more.open` and `shapes.stencilClose.<name>` for its popup actions. `shapes.filter` is the text field, `shapes.filter.clear` clears it, and `shapes.gallery` is the scroll region. `shapes.tile.<name>` remains for unambiguous names; `shapes.tileId.<master-key>` uniquely names a master (use the exact returned identifier, including escaped separators for imported masters). The window state includes `shapeFilter` and `shapeFilterCount`. This filters the active stencil only.

### `ui.click`, `ui.drag`, `ui.release`, `ui.move`, `ui.wheel`, `ui.key`, `ui.text`

Pointer and keyboard input for this window, given to egui as a person's would be, so it reaches every control. It goes
to the window and to nothing else on the desktop. Positions are screen points from the window's top left, or `element`
names a control from `ui.elements` and means its middle.

| Method | Keys |
|---|---|
| `ui.move` | `x`, `y` or `element` |
| `ui.click` | `x`, `y` or `element`; `button` (`left`, `right`, `middle`), `count` (1 to 16), `ctrl`, `shift`, `alt` |
| `ui.drag` | `x`, `y` or `element` to start; `toX`, `toY`; `steps` (1 to 256, default 8), `button`, `ctrl`, `shift`, `alt`; `hold` (`true`: the button stays down where the drag ends, so that a drag in progress can be looked at, until `ui.release`) |
| `ui.release` | `x`, `y` or `element` (optional: the pointer moves there first), `button`, `ctrl`, `shift`, `alt`: the release of a drag held by `hold` |
| `ui.wheel` | `x`, `y` or `element`; `dx`, `dy` (notches, `unit: "point"` for points); `ctrl`, `shift`, `alt`. Ctrl with the wheel zooms the page about that point |
| `ui.key` | `key` (an egui key name such as `Escape`, `Enter`, `A`, or `left`, `plus`), `ctrl`, `shift`, `alt` |
| `ui.text` | `text`, typed as it is |

The answer is `null` as soon as the events are queued. A press and its release (and a key's press and its release) reach
egui in separate frames, with the move before the press, so a control has seen the pointer over it first. In a drag the moves come
in a frame of their own between the press and the release (a widget that waits for a drag to begin, such as a tile of the Shapes
window, sees the pointer moving with the button down), and the release follows in the next frame. At most 1,024
events wait at once; a request that would go over is refused whole (`synthetic input queue limit`), and so is one with a key
it does not know, a count or a number of steps over its limit, or a name `ui.elements` did not list. Nothing is queued unless
the whole request is good.

### `ui.resize {width, height}`

Asks the desktop for an inner window size in points, each from 200 to 8,192 (the app's own smallest is 480 by 320). The
desktop may give another size: read `window` in `ui.inspect`.

### The window's own commands

`engine.execute` and `engine.commands` cover the window's commands as well as the engine's: the ones that change what
the window shows (and what is selected) and not the document. The editing commands (`shape.drop`, `shape.move` and the rest, `docs/editing-commands.md`) and `file.save` and `file.save_as` are the engine's; a command never opens a dialog, so `file.save_as` takes the path it is given. They are listed after the engine's, with the same fields. Each takes the keys its
`params` says and nothing else. **A command that acts on the selection (`operations.*`, `format.*`, `format.rotateText`, `arrange.*`, `clipboard.copy`, `clipboard.cut`, `clipboard.duplicate`) runs on the page the selection was made on, and is refused with `the shapes selected are on another page than the one shown` when another page is on view** (`edit.select` on page 1, then `page.select` of page 2, then the command, as a client can send them in one frame): shape ids belong to their page, and the command must not reach page 2's shapes of the same numbers. A frame drawn between them drops the selection instead (nothing is selected).

| Command | What it does |
|---|---|
| `view.zoom {percent, x?, y?}` | Zooms the page shown to `percent` (10 to 400), about the middle of the drawing area or the screen point `x`, `y` |
| `view.zoomIn`, `view.zoomOut` | One step of 25% in and out (Alt+F6 and Alt+Shift+F6, beside Ctrl+= and Ctrl+-; `view.fitPage` is Ctrl+Shift+W, and Command+0 on a Mac) |
| `view.fitPage`, `view.fitWidth` | The whole page in the drawing area; the page as wide as the area |
| `view.pan {dx, dy}` | Moves the page by points |
| `view.grid {on?}`, `view.rulers {on?}`, `view.guides {on?}` | Shows or hides the overlay; without `on` it toggles. Hidden guides are not drawn and cannot be selected or dragged; they still pull a drag that snaps to guides (the document's Snap & Glue settings say so, not the box). **The setting is the drawing's**: with a drawing open the box is written into it by the engine's `window.settings.set`, which makes the drawing unsaved and is no undo step (Ctrl+Z leaves the box as it was turned, and no revision moves), and a save keeps it. A batch or a `window.settings.set` sent here is shown as the box turned. An opened file starts each from its drawing window, and a drawing with no windows part, or whose windows part holds no drawing window, shows the grid, the rulers and the guides on |
| `view.connectionPoints {on?}` | The View tab's Connection Points box, a window setting: on, the connection points of the shapes in view are marked whatever the tool; off, only while the Connector tool is on or a connector's end is dragged. Without `on` it toggles. An opened file sets it from its drawing window's `ShowConnectionPoints`, off where it has none; a new drawing (`file.new`) turns it off, and Grid, Ruler and Guides on; it is held in the drawing and kept by a save, as Grid is. A shape whose `Connection` section has more than 256 rows shows no points, and a frame marks at most 4,096 |
| `view.autoConnect {on?}` | The View tab's AutoConnect box: the drawing's own setting, one engine `autoconnect.set` (no undo step: the drawing is unsaved, so a save keeps it, and Ctrl+Z leaves the box as it was turned), and without `on` the box is toggled; refused (the control is greyed out) while the program-wide setting is off |
| `window.enableAutoConnect {on?}` | The program-wide Enable AutoConnect setting (File, Options, Advanced: there is no Options page for it yet): on at first, not saved; off, no drawing shows AutoConnect arrows and the View box is greyed out |
| `window.snapGlue {tab?, close?}` | Opens the Snap & Glue dialog (the Visual Aids launcher, Alt+F9) on `General` or `Advanced`, every box from `snap.settings`; `close` closes it and sends nothing. OK (`snapglue.ok`) sends one `snap.set` with the options changed (none when nothing was; it is no undo step and marks the drawing unsaved; the View tab's Snap and Snap to Grid boxes, `ribbon.View.SnapToggle` and `ribbon.View.SnapToGrid`, and the status bar's Snap mark, `status.snap`, send one `snap.set {snap}` or `{grid}` the other way from how the document has it); Cancel and Escape send nothing. The boxes are `snapglue.option.<option>`, each named by its option in `snap.settings`; `snapglue.note` is the line under Glue to, and `snapglue.error` the sentence a refused OK leaves in the dialog. The Advanced tab's snap strength rows are `snapglue.strength.<ruler|grid|guides|points|geometry|extensions>.slider` and `.box` (a slider has an accessible name such as `Grid snap strength`, and `ui.key` and `ui.text` into a box work as in any text field), `snapglue.strength.note` the line under them; OK sends the strengths that changed in the same one `snap.set`, and a box that is no whole number of pixels from 0 to 999 refuses OK in words in `snapglue.error` |
| `window.zoomDialog {close?}` | Opens the Zoom dialog (View, Zoom, Zoom, and the status bar's zoom percentage) on Whole page, with the zoom now in the Percentage box; `close` closes it and changes nothing. It sends no engine command until OK, which runs `view.zoom {percent}` for a listed percentage (`zoom.choice.400`, `200`, `150`, `100`, `75`, `50`) or the number typed in `zoom.percent` (10 to 400, with or without a percent sign), `view.fitWidth` for `zoom.choice.page_width` and `view.fitPage` for `zoom.choice.whole_page`; a number outside the range or a word is refused in `zoom.error` and the dialog stays open. `zoom.ok` and `zoom.cancel` are its buttons (Escape is Cancel), `zoom.content` the list. The window state's `zoomDialog` is the name of the choice the dialog has now (`whole_page`, `percent`, `200` and so on), null when it is closed. It needs a drawing |
| `window.colorsDialog {target, close?}` | The Colors dialog (More Colors... in the Fill, Line and Font Color menus and the Format Shape pane's colour buttons; needs a selection): `target` is the palette's key (`fill`, `line`, `color` for the text, `stop_color`, `line_stop_color`, `fill_background` or `shadow_color`); `close` closes it and changes nothing. It opens on the Standard tab with the colour the selection has as **New** and **Current**. The elements are `colors.tab.standard`, `colors.tab.custom`, the cells `colors.standard.#rrggbb`, on the Custom tab `colors.field`, `colors.bar`, `colors.model.rgb`, `colors.model.hsl`, `colors.red`, `colors.green`, `colors.blue` (or `colors.hue`, `colors.saturation`, `colors.lightness`), `colors.hex`, then `colors.new`, `colors.current`, `colors.error`, `colors.ok` and `colors.cancel`. OK sends one `format.shape`, `format.text`, `format.gradient` or `format.shadow` with the colour as `#rrggbb` (for a text colour with a box open, `text.format` of the characters that were selected in it), makes it the newest of the Recent Colors, and closes; a refusal is said in `colors.error`. The hex box is refused in words when it is no colour; Escape is Cancel. The window state's `colorsDialog` is `{target, tab, model, new, current, hex, error}` or null, and `recentColours` is the Recent Colors, newest first |
| `window.find {mode?, what?, with?, scope?, in?, match_case?, whole_words?, action?, close?}` | Opens the Find dialog (`mode` `find`, the default, or the mode the dialog has) or the Replace dialog (`replace`), Home > Editing > Find, Ctrl+F for Find. **A floating window that does not shade the drawing area** (it is not a modal: `is_modal_open` is false, and the drawing under it takes the pointer and the keys); an open one changes to the mode asked and keeps what was typed. `what` and `with` are the text of Find what and Replace with (at most 1,024 characters), `scope` is `selection`, `page` or `all`, `in` is any of `text`, `shape_data`, `shape_name` and `user_cell` (the Find dialog's four boxes; the Replace dialog replaces shape text alone), `match_case` and `whole_words` are its options. `action` does what a button does: `next` is Find Next (`edit.find` from the match the dialog is at with a limit of 1, then `page.select` and `edit.select` to show it, and the view moves to the shape when it is out of sight), `replace` is Replace (`edit.replace` of that match, then Find Next from after the new text, or Find Next alone when the dialog is at none), `replace_all` is Replace All (one `edit.replace`, one undo step); `close: true` closes it. The answer is the window state, whose `find` is `null` when the dialog is closed and otherwise `{mode, what, with, scope, in, matchCase, wholeWords, current: the match Find Next is at, line: the Found in line or what a button did, problem: whether the line is a refusal, history: the earlier searches}`. The elements are `find.window`, `find.what`, `find.with` (Replace), `find.what.earlier` and `find.with.earlier` (the lists of earlier entries, whose rows are `find.what.history.<n>`), `find.special` and `find.special.^t`, `find.special.^l`, `find.special.^-`, `find.special.^?`, `find.scope.selection`, `find.scope.page`, `find.scope.all`, `find.in.text`, `find.in.data`, `find.in.name`, `find.in.user` (Find), `find.match_case`, `find.whole_words`, `find.found_in`, `find.next`, `find.replace` and `find.replace_all` (Replace) and `find.close`. Enter in Find what is Find Next and Escape closes the dialog. It needs a drawing |
| `window.ribbon {collapsed?}`, `window.shapes {shown?}` | Folds the ribbon to the tab row; folds the Shapes window to a strip |
| `window.tab {tab}` | Opens a ribbon tab by its label (`File` opens the File view) |
| `window.stencil {stencil}` | The Shapes window shows the masters of the stencil with this id or name (any case), from `stencil.list` (a stencil that is not open is opened first with `stencil.open {id}`; the More Shapes menu does both, and its elements are `shapes.more.<name>` for each stencil, `shapes.open.<name>` for the check mark of each that is open and `shapes.category.<name>` for the headings) |
| `window.shapeFilter {query}` | Filters the current stencil by a case-insensitive substring of master names. Processes the first 4,096 Unicode characters, maps CR/LF to spaces, removes other control characters and retains at most 128 characters. Empty query clears it. Does not change the document or history; changing stencil clears the filter. |
| `shapes.search {text?, mode?}` | Searches every stencil for the words of `text` (`stencil.search`: whole words, in any case, of the names and search words of the masters, at most 128 characters here) and shows the results as the stencil `search`, each stencil's matches under its name (the box's Enter and magnifier send it). Blank `text` clears the search and shows the stencil that was shown before it, and without `text` the words of the last search are searched again. `mode` is `all` (the default) or `any`. A refusal is the engine's, in words, and the results stay. The answer's `shapeSearch` is `null` or `{text, mode, matches, listed, complete}` |
| `window.theme {theme}` | `light` or `dark` |
| `window.backstage {page}` | Opens the File view on `Home`, `New` or `Print`, or leaves it with `close` (`Print` needs a drawing; Ctrl+P shows it too) |
| `window.templateDetail {template?, close?}` | Opens the File view on New with the detail of the template whose id `template.list` gives (any case) in place of the gallery: the elements `backstage.detail`, `backstage.detail.picture`, `backstage.detail.create` and `backstage.detail.close`; Create asks about unsaved changes and then runs `file.new {template}`, Close or `close: true` goes back to the gallery. An id that is no template is refused in words, with the ids there are |
| `print.set {printer?, copies?, collate?, two_sided?, color?, pages?, sheet?, zoom?}` | The Print view's choices, all or none: `printer` is a name of `print.printers` in the window state or `pdf`; `copies` 1 to 999; `two_sided` `default`, `off`, `long` or `short`; `color` `color` or `grayscale` (black and white, for a printer or `pdf`: the preview is drawn again in greys and the PDF has every colour as its grey); `pages` `all` or `current`; `sheet` the sheet to show, from 1; `zoom` percent from 25 to 400 or `fit` |
| `print.start {path?}` | Prints with the choices: to a printer (no `path`), or to the PDF at `path` when the destination is `pdf` (a program names the file and no dialog opens). The next frame begins it; the window state's `print.busy` and `print.last` say how it goes and how it ended |
| `window.developer {on?}` | Shows the Developer tab |
| `window.options {tab?, close?}` | Opens the Options dialog (File, Options) on `Customize Ribbon`, the one tab that works, with the Developer tab's box as the window has it; `close` closes it and sends nothing. The File view closes behind it. In `ui.elements` the sections are `options.section.<name>` (only Customize Ribbon is enabled), the Main Tabs `options.tab.<tab>` (only `options.tab.Developer` is enabled), then `options.ok` and `options.cancel`; OK sends one `window.developer` if the Developer box was changed, Cancel and Escape send nothing. The window state's `options` is the tab the dialog is open on, or null |
| `window.layerProperties {close?}` | Opens the Layer Properties dialog (Home, Editing, Layers) on the layers of the page on view, each row from `layer.list`; `close` closes it and sends nothing. The boxes are `layers.row.<n>.visible`, `.print`, `.active`, `.lock`, `.snap`, `.glue` and `.color` and the name button `layers.row.<n>.name`; `layers.new` opens the name box (`layers.new_name`, `layers.add`), `layers.remove` (and, for a layer with shapes, `layers.remove_with_shapes`, `layers.remove_keep`, `layers.remove_cancel`), `layers.rename`, `layers.colour.none` and `layers.colour.<n>`, `layers.unreferenced`, and `layers.ok`, `layers.apply` and `layers.cancel`. OK and Apply send one `command.batch` (undo step "Layer properties") of `layer.delete`, `layer.set`, `layer.add` and, when the box is ticked, `layer.delete {unreferenced}`; a refusal is said in `layers.error` and the dialog stays open |
| `window.defineShapeData {close?}` | Opens the Define Shape Data dialog (Data, Show/Hide; the shape's right-click menu, Data, Define Shape Data...) for the one shape selected, with every field from `shapedata.list` (hidden ones too); `close` closes it and sends nothing. The elements are `data.field.<n>` (the list), `data.new`, `data.delete`, `data.up`, `data.down`, the boxes `data.label`, `data.name` (a new field), `data.type` and `data.type.<id>`, `data.format`, `data.formats` and `data.format.<n>`, `data.value`, `data.prompt`, `data.sort_key`, `data.hidden`, `data.ask`, `data.lang_id` (a language tag), `data.calendar` and `data.calendar.<n>` (a date field's calendar, one of the fourteen `shapedata.types` names), `data.error`, `data.ok` and `data.cancel`. OK sends one `shapedata.define` (undo step "Define shape data") with only the boxes that were changed, and a refusal is said in the dialog, which stays open |
| `window.askShapeData {id, page?, close?}` | Opens the card a drop asks for the shape (the Shapes window runs it after a master is dropped): the fields with Ask on drop, a box or a drop-down for each (`ask.<row>`, `ask.<row>.item.<n>`), `ask.ok`, `ask.cancel` and `ask.error`. It opens nothing for a shape none of whose fields asks. OK sends one `shapedata.set` of the values changed; `close` closes the card and sends nothing |
| `window.assignLayer {close?}` | Opens the Assign to Layer dialog for the shapes selected (greyed out with nothing selected): a box for each layer, ticked, clear or mixed by how many of the shapes are on it (`assign.layer.<n>`), `assign.all`, `assign.none`, `assign.new` (`assign.new_name`, `assign.add`), `assign.preserve`, `assign.ok` and `assign.cancel`; `close` closes it and sends nothing. OK sends one `command.batch` (undo step "Assign to layer") of `layer.add` and `layer.assign` with `mode` `add` and `remove` for the boxes that changed |
| `window.containerAdd {style?}` | Insert, Diagram Parts, Container: `container.add` with the shapes selected and the page on view, or with nothing selected an empty container dropped in the middle of the view; the new container is selected, so that the Container Format tab shows. `style` is one of the ids `container.styles` lists (default `strip`). Needs a drawing |
| `window.containerDo {op, value?}` | The Container Format tab and the right-click row Container, Add to Underlying Container: `op` is `style` (`value` a style id), `heading` (`value` `top`, `right`, `bottom`, `left` or `rotate`), `margin` (`value` `none`, `narrow`, `medium`, `wide` or inches), `fit`, `auto_resize` (`value` `none`, `expand` or `fit`), `lock` (toggles), `disband` (the shapes the frames held are left selected), `select_contents` (selects the members) or `add_member` (to the container beneath each shape selected). It works on the containers among the shapes selected (`add_member` on all of them), and is refused in words with nothing selected or no container among them. In `ui.elements`: the tiles of the galleries are `menu.container.<style id>` and the rows of the menus `menu.<control id>` |
| `window.orgChartDo {op, value?, path?}` | The Org Chart tab: `op` is `layout` (`value`: one of the six layouts of `orgchart.layout`), `relayout`, `arrange` (`value`: `horizontal`, `vertical_right`, `vertical_left` or `inherit`, for the position selected), `picture` (for the position selected: with `path` the file, without it the file chooser Insert, Pictures uses), `picture_delete` or `picture_hide` (a toggle). Adds the page on view and the position selected and sends `orgchart.layout`, `orgchart.relayout`, `orgchart.arrange`, `orgchart.picture_set`, `orgchart.picture_delete` or `orgchart.picture_hide`; `select a position of the org chart first` when none is selected. Enabled where the drawing has a position |
| `window.relayout {style?, direction?, alignment?, spacing?, connector_style?, appearance?, apply_routing?, enlarge_page?}` | Design, Layout, Re-Layout Page: sends `page.relayout` with these params for the shapes selected on the page on view, or for the page when none is; what is selected stays selected. The tiles are `menu.relayout.<style>[.<direction>]` (`flowchart.top_to_bottom`, `hierarchy.left_to_right`, `compact_tree.down_right`, `radial`, `circular`, ...: 14 in all). Needs a drawing with a shape |
| `window.relayoutOptions {close?}` | The Configure Layout dialog (More Layout Options...): opens with what `page.layout` says; `close` closes it and sends nothing. The elements are `relayout.style.<kind>`, `relayout.direction.<name>`, `relayout.alignment.<name>`, `relayout.spacing`, `relayout.connector_style.<name>`, `relayout.appearance.<name>`, `relayout.apply_routing`, `relayout.enlarge`, `relayout.to_selection`, `relayout.to_page`, `relayout.preview`, `relayout.error`, `relayout.ok`, `relayout.cancel` and `relayout.apply`; `relayoutDialog` in the window state is its style while it is open |
| `window.autoSpace` | Home, Arrange, Position, Auto Space: `shape.autospace` for the shapes selected, or for the page. Needs a drawing with a shape |
| `window.spacingOptions {close?}` | Spacing Options...: a dialog of the page's two spaces (`spacing.horizontal`, `spacing.vertical`, `spacing.ok`, `spacing.cancel`, `spacing.error`; OK sends `page.spacing`); `close` closes it and sends nothing. `spacingDialog` in the window state is true while it is open |
| `arrange.moveOffPageBreaks` | Move off Page Breaks: `shape.move_off_page_breaks` for the shapes selected, or for the page |
| `arrange.avoidPageBreaks {on?}` | Avoid Page Breaks: `page.avoid_page_breaks` for the page on view, toggled without `on` |
| `view.pageBreaks {on?}` | View, Show, Page Breaks: the drawing's own box (`page_breaks.set`: a change a save keeps and no undo step), toggled without `on`; `pageBreaks` in the window state is the box, and `ui.inspect`'s canvas report says how many lines were drawn (`pageBreaks`: across the page and down it) |
| `window.insertPicture {path?}` | Insert, Illustrations, Pictures: with `path` (a PNG, JPEG, GIF, BMP, TIFF, EMF or WMF file) sends `insert.picture` for the page on view and selects the new shape, with no dialog; without it shows the desktop's file chooser on a thread of its own and inserts the file chosen the same way. `pictureDialog` in the window state is true while the chooser is up |
| `window.callout {style?}` | Insert, Diagram Parts, Callout: sends `callout.insert` with the style (the first tile when it is left out), for each shape selected on the page on view (a callout tied to it) or, with none selected, one tied to nothing at the middle of the drawing area, and selects the callouts made. The elements are the Callout control `ribbon.Insert.CalloutGallery`, its tiles `menu.callout.<style id>` (fourteen, in rows of four) and, while one callout is the one shape selected, its yellow handle `callout.tip`; a drag of the handle onto a shape sends `callout.attach` and onto bare page `callout.detach` with the place |
| `window.swimlaneDo {op, value?}` | The Cross-Functional Flowchart tab, which shows while the page on view has swimlanes, and the Swimlane rows of a lane's right-click menu: `op` is `insert` (`value` `before`, or `after`, the default: a swimlane before or after the lane selected, or at the end), `separator` (splits the phase selected, or the last), `orientation` (`value` `horizontal` or `vertical`), `rotate_label` (the lanes selected, or the first structure's) or `earlier` or `later` (the lane selected, one place along its list). Each sends the engine's `swimlane.insert`, `swimlane.insert_phase`, `swimlane.orientation`, `swimlane.rotate_label` or `swimlane.move` with the lane, the phase or the structure selected, or the page's first structure when none is, and a new lane or phase is selected. Refused in words on a page with no swimlanes. A lane dragged by its header is reordered by the engine when it is let go (`shape.move`) |
| `window.hyperlinks {close?}` | Insert, Links, Link (Cmd+K; one shape selected): opens the Hyperlinks dialog on the shape's links; `close` closes it and sends nothing. The elements are `links.row.<n>`, `links.new`, `links.delete`, `links.default`, `links.address`, `links.sub_address`, `links.page`, `links.description`, `links.ok` and `links.cancel`; OK sends one `hyperlink.set` |
| `window.followLink {id}` | Follows the link of the shape as a Ctrl+click on it does: `hyperlink.follow` says where it goes, a link of this drawing shows its page and selects its shape, and a web, FTP or mail address is handed to the system by egui's `OpenUrl` at the next frame (`openedUrl` in the window state is the last one) |
| `window.field {close?}` | Insert, Text, Field: opens the Field dialog for the text box that is open (the field goes at the caret) or the one shape selected (at the end of its text); `close` closes it and sends nothing. The elements are `field.category.<id>`, `field.field.<id>`, `field.formula`, `field.format`, `field.ok` and `field.cancel`; OK sends one `text.insert_field` |
| `window.symbols {close?}` and `window.symbol {text}` | Insert, Text, Symbol: `window.symbols` opens the Symbol dialog (the elements `symbol.set.<name>`, `symbol.char.<hex code>`, `symbol.insert` and `symbol.close`); `window.symbol` puts the characters in with no dialog, at the caret of an open text box (over the characters selected) or at the end of the text of the one shape selected, as one `text.insert` or `shape.set_text` |
| `app.about {close?}` | Shows About DiaCraft (File view, About): the octopus cover as a popup over the whole window, with the version, the build line (`0.2.0` for a build of its tag, `0.2.0-dev+<commit>` before the tag exists), the year and the licence, each licence name a link to its page. It has no timer, and goes on a click outside the links, on the panel or on Escape, each of which it uses up (nothing under the popup takes the press or the key, so the drawing, the ribbon and the shortcuts do not see it); `close` closes it at once, and closing what is not open is not an error. At its start the desktop app (unless `DIACRAFT_NO_SPLASH` is set) and the web page show the same panel with four seconds of the window's clock, held while the pointer is over it. In `ui.elements`: `splash.panel`, `splash.zone` (where the text goes), `splash.version`, `splash.git`, `splash.copyright`, `splash.licence`, `splash.link.mit`, `splash.link.apache` and `splash.link.licenses`, and in About alone `splash.trademark` (the notice: the trademark line and the notes on OpenDocument Format, LibreOffice and OpenOffice, and on ArchiMate, UML and BPMN). A press on a licence link asks egui to open its page in a new tab and leaves the popup up; the link under them, Third-party licences, runs `app.licenses` (the popup gives way to the window) |
| `app.licenses {close?}` | Shows the Third-party licences window (File view, About, Third-party licences): a modal card with `THIRD-PARTY-LICENSES.txt`, the libraries DiaCraft is built from and the licences they are under (in the web build `THIRD-PARTY-LICENSES-WEB.txt`, the crates of the web build), which the desktop window carries built in and the web window fetches when the card first opens (until it has come the card says it is loading, and if it could not be had the card says why and works without it), in a list that scrolls (the wheel, the bar, Page Up, Page Down, Home, End and the arrow keys; the rows are the file's lines broken at 100 columns). It needs no drawing. One that is open stays as it is, and a start or About panel that is up is closed. It goes on Escape (which it uses up) and on its Close button; `close` closes it at once, and closing what is not open is not an error. In `ui.elements`: `licenses.title`, `licenses.list` (or, while the text is on its way or could not be had, `licenses.notice`) and `licenses.close`; the state `licenses` says `rows`, `offset` and `text` (`"ready"`, `"waiting"` or `"failed"`, and then `error`) |
| `page.select {id}` | Shows the page with this id. `page.duplicate` (an engine command, run through the window) shows the copy it makes, as a tab chosen does |
| `tool.pointer`, `tool.connector`, `tool.text`, `tool.text_block` | The tool the primary button works with (Ctrl+1, Ctrl+3, Ctrl+2 and Ctrl+Shift+4). The Text tool is used up by the box it opens (the window state's `tool` is `pointer` again while the box is typed in); the Text Block tool stays on until Esc or the Pointer Tool |
| `tool.rectangle`, `tool.ellipse`, `tool.line`, `tool.freeform`, `tool.arc`, `tool.pencil` | The drawing tools of Home, Tools, Drawing Tools (Ctrl+8, Ctrl+9, Ctrl+6, Ctrl+5, Ctrl+7 and Ctrl+4): a mode that stays on until the Pointer Tool or Esc. A drag with one is one `shape.draw` on release (a stroke of points through the pointer's frames), and Esc in the middle drops it and sends nothing; the Drawing Tools button shows the tool used last. The window state's `drawing` is `{tool, points, continuing, closing}` while a drag with one is on (`continuing` the id of the shape whose path it goes on from, `closing` whether the release would close the path) and null otherwise |
| `tool.area_select`, `tool.lasso_select` | Home, Editing, Select, Area Select and Lasso Select (the Draw tab's Lasso Select too): a mode for the next drag, used up by one drag (the Pointer Tool is back), put down by Esc, and left on by a click that is no drag. A drag with Area Select is a net even from a press on a shape, and a drag with Lasso Select is a loop; each ends in one `select.by_region` and sends nothing that changes the drawing. `tool` in the window state is `area_select` or `lasso_select` while one is on |
| `window.selectByType {close?}` | Home, Editing, Select, Select by Type: opens the dialog of the seven types, the two roles and the layers of the page on view (the elements `select_type.type.<word>`, `select_type.role.<word>`, `select_type.layer.<ix>`, `select_type.ok`, `select_type.cancel`; `selectByType` in the window state is what is ticked, `null` when it is closed). OK is quiet until something is ticked and sends one `edit.selectByType`; `close` closes the dialog |
| `edit.selectByType {types?, roles?, layers?}` | Selects, with no dialog, the shapes of the page on view that the engine's `select.by_type` takes (the answer is the window state, and `selection` is the new selection); the engine's words refuse a call that gives none of the three lists |
| `edit.selectRegion {polygon? or rect?, partial?, add?}` | Selects what the engine's `select.by_region` takes for a polygon or a rectangle of the page on view, as a net or a loop with the pointer does; `partial` is the window's option when left out and `add` keeps what is selected |
| `window.selectPartially {on?}` | The program-wide option "Select shapes partially within area" (File, Options, Advanced): off at first and not saved, flipped when `on` is left out; `selectPartially` in the window state is its value |
| `draw.closeShape` | Draw, Create, Close Shape: `shape.close` of every shape selected, on the page shown (one `command.batch`, one undo step); greyed out with nothing selected, and a shape whose path is closed already or is no open path refuses it in the engine's words |
| `window.freeformSmoothing {percent?}` | The window's own setting of how much a Freeform stroke is smoothed before its curve is fitted: 0 to 100, 40 at first, not saved (File, Options, Advanced has no page for it yet); without `percent` the command only reads, and `freeformSmoothing` in the window state is the value in percent |
| `format.rotateText {by}` | `text.block_rotate` of the shapes selected, on the page shown, by `by` degrees counterclockwise on the page (90 is the Rotate Text button); several shapes are one `command.batch`, one undo step |
| `window.pageSetup {tab?, page?, close?}` | Opens the Page Setup dialog on `tab` (`Print Setup`, `Page Size`, `Drawing Scale` or `Page Properties`; Layout and Routing and Alt Text have no controls and are refused) for the page on view or for page `page`, or closes it with `close` and sends nothing. The dialog works on a draft and sends its commands only when its OK is pressed (`pagesetup.ok` in `ui.elements`) |
| `window.reorderPages {close?}` | Opens the Reorder Pages dialog (Ctrl+Alt+P; needs two pages that are not backgrounds, as the dialog puts those in order), or closes it with `close` |
| `window.pane {pane, shown?}` | Opens, closes or (without `shown`) toggles a pane at the right of the drawing area: `size_position`, `format_shape`, `pan_zoom`, `drawing_explorer` or `shape_data` (or the title, `Size & Position` and so on). The Shape Data pane's fields are the elements `pane.data.<row>` (a drop-down's items `pane.data.<row>.item.<n>` and, on a variable list, `pane.data.<row>.new`) and its button `pane.shape_data.define`; the right-click menu's row is `pane.shape_data.menu.define`. A pane opens in a space of its own at the bottom of the stack |
| `window.paneDock {pane, with?}` | Puts a pane (opened if it is not) in the space of another pane that is open, as a tab along the bottom, or without `with` in a space of its own at the bottom of the stack |
| `window.paneTab {pane}` | Brings an open pane that shares a space to the top of the space |
| `window.paneMove {pane, up}` | Moves the space of an open pane one place up (`up` true) or down the stack |
| `window.explorerBranch {branch, open?}` | Folds or unfolds a branch of Drawing Explorer: `foreground_pages`, `background_pages`, `styles`, `masters`, `fill_patterns`, `line_patterns` or `line_ends` (or its title); without `open` it is toggled |
| `window.newComment {id?, page?}` | Opens the Comments pane with a box for a comment on the shape `id`, else the shape selected, else the page; the comment is made by `comment.add` when it is posted |
| `window.showComment {comment}` | Shows a comment: its page, its shape selected, its card open in the Comments pane |
| `window.stepComment {forward?}` | Goes to the next (default) or the previous comment, from the one that is open (`comment.next`, `comment.previous`) |
| `window.textDialog {tab?, close?}` | Opens the Text dialog (F11, the launchers of the Font and Paragraph groups) on `Font`, `Paragraph` or `Text Block` for the shapes selected, or for the characters selected in the open text box; `close` closes it and sends nothing. OK (`textdialog.ok`) sends one `text.format` with what was changed |
| `edit.select {ids}` | Selects shapes of the page shown by `ID` (an empty list clears the selection); an `ID` the page does not have is ignored |
| `edit.selectAll` | Selects every shape of the page shown (Ctrl+A) |
| `format.shape {fill?, line?, weight?, pattern?, begin_arrow?, end_arrow?, rounding?, fill_transparency?, line_transparency?, fill_pattern?, fill_background?, line_cap?, compound?}` | `shape.format` of the shapes selected, on the page shown (a colour as `#RRGGBB` or `none`); `begin_arrow` and `end_arrow` take 0 to 45; the transparencies are percent, `fill_pattern` 1 to 24, `line_cap` `round`, `flat` or `square` |
| `format.gradient {...}` | `shape.gradient` of the shapes selected: `target` (`fill` or `line`), `on`, `kind` with `origin`, `direction`, `angle` (degrees), `rotate_with_shape` and `stops`; and the edit of one stop of the gradient the shape has, which sends the whole list: `stop` (the one the Format Shape pane has selected when it is left out), `stop_color` (`line_stop_color` for the line), `stop_position` (0 to 1), `stop_transparency` (percent), `add_stop` and `remove_stop` |
| `format.shadow {...}` | `shape.shadow` of the shapes selected: `on`, `preset` (`none`, `bottom_right`, `bottom`, `bottom_left`, `right`, `center`, `left`, `top_right`, `top`, `top_left`), `color` (or `shadow_color`), `transparency`, `size`, `blur`, `angle`, `distance` |
| `format.quickStyle {row?, column?, variant?}` | `shape.quick_style` of the shapes selected: a Theme Style (`row` 1 to 6 and `column` 1 to 7) or a Variant Style (`variant` 0 to 3); refused in words while the page has no theme |
| `format.options {tab?}` | Opens the Format Shape pane on `Fill & Line` (the default) or `Effects` for the shapes selected; refused while nothing is selected. Fill Options..., Line Options... and More Arrows... send it |
| `format.painter {on?, keep?, to?}` | The Format Painter (Ctrl+Shift+P): `on` true picks up the look of the first shape selected (with two shapes or more the others are painted at once), false puts it down, and without `on` it toggles; `keep` true keeps it on after it paints; `to` (an array of shape ids) paints those shapes as a press on each would, and needs the painter on. The window state's `formatPainter` is `{keep, from}` while it is on, else null |
| `format.text {font?, size?, bold?, italic?, underline?, strike?, color?, align?, valign?, bullet?, indent?, case?, direction?, vertical?}` | `text.format` of the shapes selected, or of the characters selected in the text box that is open (the box stays open) |
| `format.toggle {what}`, `format.size {step}` | Bold, Italic, Underline, Strikethrough, Bullets, Vertical Text, All Caps or Small Caps (`bold`, `italic`, `underline`, `strike`, `bullets`, `vertical`, `all_caps`, `small_caps`), off when every shape has it and on otherwise; Increase or Decrease Font Size (`up`, `down`) |
| `arrange.rotate {by}`, `arrange.flip {axis}`, `arrange.order {to}`, `arrange.align {how}` | `shape.rotate`, `shape.flip`, `shape.order` and `shape.align` of the shapes selected (the first selected is the one `arrange.align` moves the others to) |
| `clipboard.copy`, `clipboard.cut` | `edit.copy` and `edit.cut` of the shapes selected, on the page shown (a cut is one undo step and leaves nothing selected); the text of their labels goes to the system clipboard. Refused while a text box is open |
| `clipboard.paste {x?, y?}` | `edit.paste` on the page shown, at `x` and `y` (inches, both or neither; without them, a quarter inch right and down for each paste); the shapes pasted are selected. Refused while the clipboard is empty or a text box is open |
| `clipboard.duplicate {dx?, dy?}` | `shape.duplicate` of the shapes selected (a quarter inch by default; the clipboard is untouched); the copies are selected |
| `shapes.openStencil {path?}`, `shapes.closeStencil {id}` | `stencil.open` of the file at `path` and `stencil.close` of the stencil `id`, and the stencil opened is the one the Shapes window shows; without a `path` the desktop app shows its file dialog (refused where there is none). A stencil opened by `engine.execute stencil.open` is listed and shown too |
| `arrange.changeShape {master, stencil?}` | `shape.change` of the shapes selected, on the page they are on, to the master (of the stencil `stencil`, if one is named): Home, Editing, Change Shape. Greyed out with nothing selected and while the document's `DocLockReplace` is on; the shapes keep their `ID`s, so the selection stays; a shape that could not be changed is said in the status bar. In `ui.elements` the gallery's tiles are `ribbon.Home.changeShape.<master>` while the menu is open |
| `arrange.group`, `arrange.ungroup` | `shape.group` of two shapes or more selected, in one list (the group is selected); `shape.ungroup` of the groups selected (their members are selected) |
| `operations.union`, `operations.combine`, `operations.fragment`, `operations.intersect`, `operations.subtract` | `shape.union`, `shape.combine`, `shape.fragment`, `shape.intersect` and `shape.subtract` of the shapes selected, on the page shown, in the order the window keeps the selection in (the first selected is the first shape; a rubber band is back to front); no parameters. Enabled when `draw::check_shape_ops` allows the operation for the selection, and a refusal is the engine's words (`nothing is selected`, `Union needs two shapes at least ...`, `shape 1 cannot be deleted (LockDelete) ...`). One undo step; nothing is selected after it. They are the Developer tab's Operations menu |

They answer with the window's state: `{tab, ribbonCollapsed, shapesCollapsed, stencil, grid, rulers, guides, connectionPoints, guide, page, zoomPercent, backstage,
theme, pageRect, pointsPerInch, tool, selection, editingText, dirty, print, drag, lastSnap, splash, licenses, autoConnect, drawing, freeformSmoothing, selectPartially, selectByType, hyperlinks, field, symbols, pictureDialog, openedUrl, shapeData, container, relayoutDialog, spacingDialog, pageBreaks}` (`container` is `{selected, heldBy}`: the `ID`s of the containers among the shapes selected, which bring the Container Format tab, and for each shape selected that is no container the containers that hold it, which light) (`splash` is `start` while the popup at the start is up, `about` for the one `app.about` shows, else null; `licenses` is `{rows, offset}` while the Third-party licences window is open, the rows of its list and where it is scrolled to in points, else null) (`guides` is the View tab's Guides box, `connectionPoints` its Connection Points box, `guide` the `ID` of the guide selected or null; `drag` is where the drag in progress would land, `{delta, sized, turned, corners, guideAt, snap, marks}` or null, `snap` saying whether the release sends `snap`, `turned` the angle a turn by the rotation handle comes to, in degrees, `corners` the Text tool's drag as `[[x, y], [x, y]]` (the corner the press landed at, and the pointer's, in inches: the box `text.box` is sent), and `marks` the list of what the preview draws, each `{mark: "line" | "gap" | "place" | "size" | "angle", ...}` with its segment (`from`, `to`) and its `label` where it has one; `lastSnap` is the `snap` of the answer of the last command a drag sent, with `held`, or null) (`print` is the Print view's state: `{destination, copies, collate, two_sided, color, pages, sheet, sheets, zoom, plan_error, printers, service, busy, last, preview: {requested, received, drawn, lost}}`; `pageRect` is where the page is on the screen in points, `[left, top, width, height]`, and `pointsPerInch` the zoom as points to a paper inch: what a client needs to aim a drag at a place of the page; `dirty` is whether there are changes not saved), which `ui.inspect` also carries under `ui`. The `view.*` commands and `page.select` need a document; a new drawing
(`file.new`) resets the view. None of them is an edit: they do not move `revision` or the undo history. The four boxes `view.grid`, `view.rulers`, `view.guides` and `view.connectionPoints` also make the drawing unsaved (`dirty`), as the engine's `window.settings.set` does.

### `app.quit`

Answers `{"ok": true, "result": null}`, and about 150 ms later closes the window, so the answer leaves the process
first. The app then exits with status 0.

## The logger

Log records go to standard error and to `<config>/diacraft/logs/diacraft.log`, where `<config>` is
`$XDG_CONFIG_HOME` or `~/.config` (Linux and the BSDs), `~/Library/Application Support` (macOS) or `%APPDATA%`
(Windows). Each start moves the log to `diacraft.1.log` and that to `diacraft.2.log`, so two earlier runs are kept,
and a log file stops growing at 16 MiB. With `DIACRAFT_NO_PREFS` set there is no log file. `RUST_LOG` sets the
levels (`debug`, `warn,diacraft_engine=trace`); the default is `info` for DiaCraft's crates and `warn` for the rest.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-25 | trivial | `splash.trademark` names the notes on ArchiMate, UML and BPMN too |
| 2026-11-23 | trivial | Sentences reworded so that each reads whole on its own; nothing they say changes |
| 2026-11-23 | minor | The examples say 0.2.0, and `app.about` shows the build line (the plain version for a build of its tag) where it said the git release |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | `splash.trademark` is the whole notice (the trademark line and the notes on OpenDocument Format, LibreOffice and OpenOffice) |
| 2026-11-23 | minor | `ui.elements` has `splash.trademark` while About DiaCraft is up (the trademark line; the start splash has none) |
| 2026-11-23 | minor | `window.orgChartDo` and the Org Chart tab's elements; the window state has `orgChart` |
| 2026-11-23 | minor | `shapes.search`, and `shapeSearch` in the window state |
| 2026-11-23 | minor | The window command `window.swimlaneDo` has its row |
| 2026-11-23 | minor | The palette's elements for the ten theme columns: a swatch of the third or fourth column is `palette.<key>.theme:<colour>[:<shade>]@<place>` |
| 2026-11-22 | minor | `window.colorsDialog` and its elements, the palette's elements and the window state's `colorsDialog`, `recentColours` and `themePreview` |
| 2026-11-22 | minor | `view.fitPage`'s key on a Mac |
| 2026-11-22 | minor | `window.templateDetail`, the template detail's elements, the More Shapes check marks and headings, and the zoom keys |
| 2026-11-22 | minor | `window.defineShapeData`: `data.lang_id` is a language tag, `data.calendar` and `data.calendar.<n>` a date field's calendar, and OK sends only the boxes changed |
| 2026-11-22 | minor | A busy port says that on macOS a listener on every address does not hold the port against the app |
| 2026-11-23 | minor | The state `licenses` has `text` (`ready`, `waiting` or `failed`) and `error`, and `ui.elements` has `licenses.notice` (the card's place for the list while it is on its way or could not be had) and `splash.waiting` (the splash panel while its picture is on its way): the web build fetches both |
| 2026-11-22 | minor | `app.licenses` shows `THIRD-PARTY-LICENSES-WEB.txt` in the web build (the desktop programs show `THIRD-PARTY-LICENSES.txt`) |
| 2026-11-22 | minor | The window commands `window.relayout`, `window.relayoutOptions`, `window.autoSpace`, `window.spacingOptions`, `arrange.moveOffPageBreaks`, `arrange.avoidPageBreaks` and `view.pageBreaks`, the `relayoutDialog`, `spacingDialog` and `pageBreaks` keys of the window state, and the elements of the gallery and the two dialogs |
| 2026-11-22 | minor | The window commands `format.gradient`, `format.shadow` and `format.quickStyle`; `format.shape` takes the transparency, pattern, cap and compound parameters; the elements of the Format Shape pane's gradient, pattern, line and Shadow controls and of the Quick Styles gallery |
| 2026-11-22 | minor | `window.defineShapeData` and `window.askShapeData`, `shape_data` among the panes of `window.pane`; the elements of the Shape Data pane and of the Define Shape Data dialog and the Ask on drop card; `shapeData` in the window state |
| 2026-11-22 | minor | The window commands `window.containerAdd {style?, heading?}` and `window.containerDo {op, value?}` run the engine's container commands with the selection, the page and the middle of the view; the window state has a `container` key (the settings of the container selected) and the Container Format tab shows when one is |
| 2026-11-22 | minor | `saveNote` in `ui.inspect` ends with the words of the check after the save; the result of `file.save_as` has `checked`. |
| 2026-11-22 | minor | `window.callout` and the elements of the Callout gallery and the yellow handle |
| 2026-11-22 | minor | `arrange.changeShape` and the elements of the Change Shape gallery |
| 2026-11-22 | minor | `window.insertPicture`, `window.hyperlinks`, `window.followLink`, `window.field`, `window.symbols` and `window.symbol` with the elements of the three dialogs, and `hyperlinks`, `field`, `symbols`, `pictureDialog` and `openedUrl` in the window state |
| 2026-11-22 | minor | The elements `autoconnect.knob.right`, `.left`, `.up` and `.down` are gone, the circle handles of AutoConnect's arrows being the web app's |
| 2026-11-22 | minor | `saveNote` in `ui.inspect`; the elements `save-note`, `save-note.dismiss` and `macros.save_as_enabled` |
| 2026-11-22 | minor | `window.layerProperties` and `window.assignLayer` with the elements of the two dialogs; `layerProperties` and `assignLayer` in the window state |
| 2026-11-22 | minor | `format.text` takes `bullet`, `indent`, `case`, `direction` and `vertical`, and `format.toggle` takes `bullets`, `vertical`, `all_caps` and `small_caps` |
| 2026-11-22 | minor | `window.find` and `find` in the window state; the elements `find.*` of the Find and Replace dialogs, which float over the drawing area and are not modals |
| 2026-11-22 | minor | `tool.rectangle`, `tool.ellipse`, `tool.line`, `tool.freeform`, `tool.arc` and `tool.pencil`; `draw.closeShape`; `window.freeformSmoothing`; `drawing` and `freeformSmoothing` in the window state |
| 2026-11-22 | minor | The elements `autoconnect.knob.right`, `.left`, `.up` and `.down`, the press areas of the circle handles beside AutoConnect's arrows |
| 2026-11-22 | minor | `legs` in the window state (`connector`, `handles`, `reroute`, `dragging`) and the elements `connector.leg.<n>` |
| 2026-11-22 | minor | `tool.area_select`, `tool.lasso_select`, `window.selectByType`, `edit.selectByType`, `edit.selectRegion` and `window.selectPartially`; `selectPartially` and `selectByType` in the window state; the elements `select_type.*` and `options.partial` |
| 2026-11-22 | minor | The `view.autoConnect` and `window.snapGlue` rows say the AutoConnect box and the Snap & Glue options (the dialog's OK, the View tab's Snap, Snap to Grid and Dynamic Grid boxes and the status bar's Snap mark) are no undo step and mark the drawing unsaved |
| 2026-11-22 | minor | The `view.*` row: a drawing whose windows part holds no drawing window shows the grid, the rulers and the guides on |
| 2026-11-22 | minor | `app.licenses {close?}` (the Third-party licences window: the file `THIRD-PARTY-LICENSES.txt` in a list that scrolls, run by the About panel's new link), `licenses` in the window state, the elements `licenses.*` and `splash.link.licenses` |
| 2026-11-22 | minor | `view.autoConnect` and `window.enableAutoConnect`; `autoConnect` in the window state (`program`, `drawing`, `arrows`, `flyout`); the elements `autoconnect.arrow.right`, `.left`, `.up` and `.down` (the arrow's press area, while the arrows show), `autoconnect.flyout` and `autoconnect.flyout.<n>` (the flyout and its tiles, while it is open) |
| 2026-11-22 | minor | `view.grid`, `view.rulers`, `view.guides` and `view.connectionPoints` make the drawing unsaved and are no undo step, and no revision moves for them |
| 2026-11-22 | minor | `app.about {close?}` (the splash popup and About DiaCraft, no timer, used up by a click or Escape), `splash` in the window state, the elements `splash.*` and `backstage.About` |
| 2026-11-22 | minor | `view.grid`, `view.rulers`, `view.guides` and `view.connectionPoints` write the box into the drawing through the engine's `window.settings.set` (one undo step, the drawing unsaved, a save keeps it), and the window shows an undo of one, a batch or a `window.settings.set` sent here; a drawing with no windows part shows the grid, the rulers and the guides on |
| 2026-11-21 | minor | `print.set {color: grayscale}` draws the preview and the PDF in greys; the `print.color.color` and `print.color.grayscale` elements are enabled for a PDF too |
| 2026-11-21 | minor | `snapglue.strength.<kind>.slider`, `snapglue.strength.<kind>.box` and `snapglue.strength.note` in the Snap & Glue dialog's Advanced tab; `snap.set` and `snap.settings` carry the six strengths |
| 2026-11-21 | minor | `page.duplicate` shows the copy it makes; `window.reorderPages` needs two pages that are not backgrounds; the Page Setup dialog's Page Properties elements `pagesetup.type.foreground`, `pagesetup.type.background` and `pagesetup.background` |
| 2026-11-21 | minor | `notice` goes eight seconds after it is first shown or at the next command that changes something; a print's or an export's notice stays until the work ends |
| 2026-11-21 | minor | The window commands `format.painter {on?, keep?, to?}` and `format.options {tab?}`; `formatPainter` in the window state; the element names of File > New (`backstage.tile.blank`, `backstage.tile.template.<id>`, `backstage.heading.<name>`) |
| 2026-11-21 | minor | `window.zoomDialog` and `zoomDialog` in the window state; the elements of the Zoom dialog, the status bar's Snap mark `status.snap`, the View tab's `SnapToggle` and `SnapToGrid`; `status.zoom_percent` opens the Zoom dialog |
| 2026-11-21 | minor | The window's answer of `file.open` has `stencil_notes` (a list of sentences) when a stencil it had open was not opened again in the new drawing; a file opened while a cancelled read is still parsing waits for it, and is refused in words as `out_of_memory` only where the two do not fit together |
| 2026-11-20 | minor | `drag` is `{delta, sized, turned, corners, guideAt, snap, marks}`, `corners` being the box the Text tool's drag makes |
| 2026-11-20 | minor | `drag` is `{delta, sized, turned, guideAt, snap, marks}` and `marks` is the list of the marks the preview draws (`{mark: "line" | "gap" | "place" | "size" | "angle", ...}` with each one's segment and label), where it was their number |
| 2026-11-09 | minor | The window's commands that act on the selection run on the page the selection was made on and are refused in words when another page is on view |
| 2026-11-09 | minor | `operations.union`, `operations.combine`, `operations.fragment`, `operations.intersect` and `operations.subtract` (the shape operations of the Developer tab's Operations menu on the shapes selected); `window.options`; `options` in the window state; the elements of the Options dialog (`options.section.<name>`, `options.tab.<tab>`, `options.ok`, `options.cancel`, `options.note`) |
| 2026-11-08 | minor | `file.new` resets `view.connectionPoints`, `view.grid`, `view.rulers` and `view.guides` to a new drawing's defaults; the elements `snapglue.note` and `snapglue.error` |
| 2026-11-08 | minor | `view.connectionPoints` and `window.snapGlue`; `connectionPoints` and `snapGlue` in the window state; the elements of the Snap & Glue dialog (`snapglue.tab.General`, `snapglue.tab.Advanced`, `snapglue.option.<option>`, `snapglue.content`, `snapglue.ok`, `snapglue.cancel`) |
| 2026-11-08 | minor | `ui.drag {hold}` leaves the button down and `ui.release` lets it go; `view.guides`; `guides`, `guide`, `drag` and `lastSnap` in the window state; the elements `canvas.ruler.top`, `canvas.ruler.left` and `canvas.guide.<id>` |
| 2026-11-07 | minor | `file.new` in `engine.execute` with a `template` that is the path of a file is read on a thread and answered when the reading ends (L3 of the task review) |
| 2026-11-07 | minor | `window.pane`, `window.paneDock`, `window.paneTab`, `window.paneMove`, `window.explorerBranch` and `window.textDialog`; `panes`, `explorerOpen` and `textDialog` in the window state; the element names of the dock and its panes (`panes`, `pane.<pane>`, `pane.<pane>.title`, `.menu` and `.close`, `pane.tab.<pane>`, `pane.menu.*`, `pane.size.*`, `pane.format.*`, `pane.pan_zoom.thumb`, `.view` and `.slider`, `explorer.branch.*`, `explorer.page.<id>`) and of the Text dialog (`textdialog.*`) |
| 2026-11-07 | minor | `tool.text`, `tool.text_block`, `format.rotateText`, `window.pageSetup` and `window.reorderPages`; `textBlock`, `pageSetup` and `reorderPages` in the window state; the element names of the Page Setup dialog (`pagesetup.*`), the page tabs' menu (`page-menu.*`) and the Reorder Pages dialog (`reorder.*`) |
| 2026-11-07 | minor | `print.set` and `print.start`; the `Print` page of `window.backstage`; `print` in the window state; the controls of the Print view in `ui.elements` (`print.start`, `print.printer`, `print.copies`, `print.pages.<all or current>`, `print.collate`, `print.duplex`, `print.color.<color or grayscale>`, `print.preview`, `print.sheet`, `print.map.<n>`, `print.prev`, `print.next`, `print.zoomIn`, `print.zoomOut`, `print.fit`, `print.cancel`) |
| 2026-11-07 | minor | `shapes.openStencil` and `shapes.closeStencil` open and close a stencil file in the Shapes window |
| 2026-11-07 | minor | `clipboard.copy`, `clipboard.cut`, `clipboard.paste`, `clipboard.duplicate`, `arrange.group` and `arrange.ungroup` act on the selection and select what they made |
| 2026-11-07 | minor | `format.shape`, `format.text`, `format.toggle`, `format.size`, `arrange.rotate`, `arrange.flip`, `arrange.order` and `arrange.align` act on the selection; a window command that changes the document or the page ends a text box that is open (so a client that wants to read the box leaves `ui.inspect` for it) |
| 2026-11-01 | minor | `window.stencil`, and `stencil` in the window state; element names of the Shapes window's stencil rows and flyout, and of the card's Cancel button |
| 2026-11-01 | minor | `tool.pointer`, `tool.connector`, `edit.select` and `edit.selectAll`; the window state answers the tool, the selection, `pageRect`, `pointsPerInch`, `editingText` and `dirty`; in a drag the moves and the release reach egui in separate frames; `app.quit` is not asked about unsaved changes |
| 2026-11-01 | minor | `panics.ui` counts drawing panics and lost shapes; `ui.inspect` answers `picture.lost` and `picture.caughtPanics`; the element `canvas.lost` |
| 2026-11-01 | minor | `file.open` in `engine.execute` is read on a thread and answered when the reading ends; `ui.inspect` also answers `opening`, `openError`, `picture` and `findings`; the `page-tab.<id>` controls are the file's pages in tab order |
| 2026-11-01 | minor | `ui.elements` (the controls the last frame drew, by name), `ui.click`, `ui.drag`, `ui.move`, `ui.wheel`, `ui.key` and `ui.text` (input for this window, a press and its release in separate frames, at most 1,024 events queued), `ui.resize`, and the window's own commands (`view.*`, `window.*`, `page.select`) in `engine.execute` and `engine.commands`; `ui.inspect` also answers `title` and `ui` |
| 2026-10-31 | minor | Final review: the lines a client queued behind a bad line are discarded and never run (the page said dropped unread, though the server does read them for a moment so that its refusal arrives); the protocol did not change |
| 2026-10-31 | minor | Sentences rewritten where they matched the sibling craft app's page too closely (see `docs/provenance.md`); the examples use ids 7 and 8; the protocol did not change |
| 2026-10-31 | minor | `ui.screenshot` refuses an existing path that is not a regular file and writes the file whole; its answer was seen on a real window (1472 by 828 pixels, the PNG read back) |
| 2026-10-31 | minor | Created: the protocol, its limits, the six M0 methods, the busy-port and hidden-window behaviour; the first real run answered `ui.screenshot` with the no-frame error behind a locked screen |
