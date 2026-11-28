# The MCP server

`diacraft-cli mcp` makes DiaCraft's commands available to an agent as [Model Context Protocol](https://modelcontextprotocol.io)
tools, over standard input and output. It is a different way to ask for what the menus and the
[control channel](control-protocol.md) ask for: every tool ends in the same command registry, so a command does the same
thing however it was asked for. Nothing here claims what the reference app does.

The server is a single thread with no async runtime. It reads a line, handles it, writes the reply and reads the next, so
replies come in the order of the requests.

## Starting it

```
diacraft-cli mcp --headless               # a session of its own, no window
diacraft-cli mcp --connect 127.0.0.1:7997 # the running app
diacraft-cli mcp                          # the app if DiaCraft answers at 127.0.0.1:7997, else headless
```

A client that starts servers from a configuration lists it as a command:

```json
{"mcpServers": {"diacraft": {"command": "diacraft-cli", "args": ["mcp", "--headless"]}}}
```

| Backend | What it is | The `ui_*` tools |
|---|---|---|
| headless | A session in the server's process. It starts with no document: `command_run` `file.new` makes one. It is gone when the server ends. | Answer `isError`: they need the app. |
| the app | The window started with `diacraft --control 7997`, reached through its control channel. What the agent does is what the person sees, and the person's document is the one it changes. | Work. |

**The server only drives DiaCraft.** The port the documentation names, 7997, is not reserved: another craft app, or anything
else, may be listening there. Before it sends anything else on a connection, the server asks `ui.inspect` and goes on only
if the answer says `"app": "diacraft"`. With `--connect` a different answer is an error (status 1, naming what answered);
in the default mode it is a reason to run headless, which the server says on standard error. This is checked again each time
the server opens a new connection, not only the first.

The app closes a connection that has been idle for five minutes. The server notices before its next call, opens a new
connection, and checks again who answers. If the app has quit, or something else is on the port, the call is an error that
says `nothing was sent`.

**A reconnect is not invisible.** If the app that answers the new connection is DiaCraft, the next command goes to it, and the
server logs a warning on standard error saying the connection was opened again. An app that was restarted in between has a
different document, and a command that names a page by its id (`page.rename`) can then change another page than the agent
meant. After a warning, or after any pause of five minutes or more, look at the document (`doc_inspect`) before using ids
from earlier. The server does not do that for the agent: it cannot tell a restarted app from one that did not restart.

The control channel has no password: use `--control` only while an agent is driving, as
[its page](control-protocol.md) says.

## Keeping files to the folders you grant

`diacraft-cli mcp --headless --automation-read-root DIR --automation-write-root DIR` keeps what the agent's commands read and write to the folders you name (gaps A2 and A5). Each flag may be given more than once (`--flag DIR` or `--flag=DIR`, at most 64 of a kind), and **a kind you leave out grants no access of its kind**: a read root alone lets an agent open files and never save one. A folder that is not there, or is not a folder, is a mistake in the command line (status 2), and the server does not start.

| Flag | What it grants |
|---|---|
| `--automation-read-root DIR` | The reads: `file.open`, `file.new` with a template file, `stencil.open`, `insert.picture` and `orgchart.picture_set`. |
| `--automation-write-root DIR` | The writes: `file.save` (the drawing's own file), `file.save_as`, `file.export`, `file.print`, and `file.check_compatibility`, which asks what a save to a path would do. |

`DIACRAFT_AUTOMATION_READ_ROOT` and `DIACRAFT_AUTOMATION_WRITE_ROOT` give the folders of a kind that has no flag, as a list written as `PATH` is (`:` between folders, `;` on Windows; an empty entry is left out, since `PATH` reads it as the current folder). They are for a client whose settings have room for an environment and none for arguments (a bundle's `env`). A flag of a kind wins over the variable of that kind.

**How a path is judged.**

- It is made absolute against the server's working folder, its `..` and every link are followed (symbolic links, and on Windows junctions), and the real path is compared with the roots part by part: `/work2` is not inside `/work`. A file that is not there yet is judged by its nearest folder that is, and the rest of its path must be plain names. A link inside a root that leads out of it is outside; a root that is itself a link is the folder it leads to (taken once, when the server starts); a `..` below a folder that is not there (on Windows too, where the system would otherwise fold `nope\..` away unseen), a link that leads nowhere and a loop of links are refused, and so is a path of Windows' reserved device names.
- **What is opened is the real path**, not the name given, and a read looks at the handle it opened before it reads a byte, so a link put in the path's place after the check is refused. On Linux the system says which file the handle is (`/proc/self/fd`) and its answer is final: a handle it places outside the roots is refused. Elsewhere the handle is compared with the file now at the checked path by the file's identity: its device and inode on Unix systems, and on Windows its volume serial number and file index, so another file of the same size and modified time put in the place is refused, and so is a link put there after the check that the path has none (it is looked at as itself and never followed). **A write is judged again just before the file is made, at the real path of its folder and its own name, and a link that stands at the name by then is refused**: a save never lands on the file a link leads to.
- A path that is empty, holds a NUL, is longer than 32 KiB or has more than 4,096 parts is refused before the file system is asked anything, and the walk up from a path that is not there is at most two calls to a part.

**A refusal is in words, and a tool result with `isError`.** It names the rule and the flag, and holds neither the path nor anything of what is there: `automation path rejected: the file is outside the folders granted for reading (--automation-read-root)`. A path outside every root is told as outside whether or not anything is there, so it cannot be used to look about the machine; one inside a root that cannot be resolved says so (`the path cannot be resolved to a place inside the folders granted for reading ...`). A kind with no root says `automation filesystem access is not granted: no folder may be written (grant one with --automation-write-root)`. A refused command changes nothing, in the drawing or on the disk.

**The client is told.** `initialize`'s `instructions` name the folders (`headless (no window); files are kept to the folders granted: read A; write B`), or say `files are not limited to any folder`, and the line the server writes on standard error says the same.

**What it keeps to, and what it does not.**

- It keeps a session of this process (`--headless`, or the default mode when no app answers). **`--connect` with a root flag is an error** (status 2): the app keeps to the roots it was started with (`diacraft --control 7997 --automation-read-root DIR --automation-write-root DIR`, [Control protocol](control-protocol.md)), and `ui_inspect` shows them (`automation`). The default mode with a root flag, or a variable, **stops** (status 1) if the app answers, rather than use an app it cannot keep: give `--headless`.
- **The folders are the flags' and the variables' alone.** The server does not ask the client for its roots (the protocol's `roots/list`) and does not read the roots a client announces: what a client grants in its own settings keeps nothing here.
- **With neither a flag nor a variable nothing is limited**, as before: any path the user can reach. To say so with one kind given, give `/` (and each drive's root on Windows) for the other.
- `diacraft-cli run`, `export`, `convert` and `roundtrip` have no roots: they read and write the paths their person types.
- A write is checked, then made. A program that can swap a folder for a link between the two could still win that race on a write, and the roots are no defence against another program on the computer that already writes in a granted folder; a read looks at what it opened, which closes it for reads on Linux (the system's own answer for the handle) and narrows it elsewhere (the file's identity, looked at before and after the check that the path has no link on the way: a folder on the way swapped for a link and back between those looks could still pass, as the standard library gives no real path for a handle).

## Installing it from a bundle or a directory

A release holds six **MCP bundles** (`.mcpb` files, the [MCP Bundle format](https://github.com/modelcontextprotocol/mcpb)), one for each system and CPU the downloads are built for:
`diacraft-mcp-<version>-linux-x86_64.mcpb`, `-linux-aarch64`, `-windows-x64`, `-windows-arm64`, `-macos-arm64` and `-macos-x86_64`. The format names the system a bundle is for and has no word
for a CPU, so each CPU has a file of its own: take the one that says yours. An app that installs MCP servers takes the file (open it with the app), and a directory of MCP servers lists it;
either way the person writes no configuration, and the app starts the server as the bundle's manifest says.

A bundle is a zip. It holds `manifest.json` (format 0.3: the name `diacraft`, the version, a `binary` server whose command is `${__dirname}/server/diacraft-cli` with the argument `mcp`, the one system it
is for, the six tools below, the licence `MIT OR Apache-2.0`, the home page and the repository), `icon.png`, the program `server/diacraft-cli` (`.exe` on Windows) and the five licence files that every
download holds. It holds the command line and nothing else: not the window. The server it starts is `diacraft-cli mcp`, in the default mode (above): a session of its own with no document, or the
running app's window if the app was started with `--control` and answers at `127.0.0.1:7997`. Install the app from the site to see a drawing in a window.

- **What an agent given the bundle can do.** Everything this page says the server can do: `file.open`, `file.save` and the rest read and write any path the server may reach (gaps A2 and A5), **unless
  you keep it to folders** ([above](#keeping-files-to-the-folders-you-grant)): the bundle starts `diacraft-cli mcp`, so a client that takes arguments or an environment for a server can add
  `--headless --automation-read-root DIR --automation-write-root DIR`, or set the two variables. Give a bundle with no folders only to an agent that you trust with your files. The server connects to nothing but the DiaCraft app on the same computer.
- **The files are not signed.** The macOS program inside is signed ad hoc, as the macOS download's is; the Windows program is not signed. Check a bundle against `SHA256SUMS.txt` of its release
  (`sha256sum --check --ignore-missing SHA256SUMS.txt`), which also holds what the registry's entry below names.
- **The official MCP Registry** (registry.modelcontextprotocol.io) lists the server as `app.diacraft/diacraft`, the reverse of the domain `diacraft.app`, which the file
  `https://diacraft.app/.well-known/mcp-registry-auth` proves ([Website](website.md), "The registry's proof of the domain"). The entry is made from a release's `SHA256SUMS.txt` by
  `cargo xtask mcp-server-json` and sent by `mcp-publisher publish` after the release is published, by hand, once for each release ([Releasing](releasing.md), "Publish the MCP server to the
  registry"). It lists the six bundles as six packages of type `mcpb`, each by the address of its file in the release and its SHA-256; the registry's entry has no field for a system or a CPU, so a
  client tells them by the file's name.
- **What was tried.** The project ran the program of a bundle as its manifest says (`packaging/mcp-bundle-check.sh`: unpacked, `--version`, `initialize` and `tools/list` on its standard input) and read
  the manifest against the format's published schema. It has not installed a bundle in an app that installs them, or published to the registry.

## The protocol

Newline-delimited JSON-RPC 2.0: a message is a line, and so is a reply. The revision is `2025-06-18`; a client that asks for
`2025-03-26` or `2024-11-05` gets the one it asked for (the messages handled are the same), and one that asks for any other
gets `2025-06-18`. Only these are answered:

| Method | Reply |
|---|---|
| `initialize` | The revision, `{"tools": {"listChanged": false}}` as the only capability, `serverInfo` (`diacraft`) and short instructions that name the backend and, for a session of this process, the folders its files are kept to. |
| `ping` | `{}` |
| `tools/list` | The tools, below, with their input schemas. |
| `tools/call` | The tool's result. |

Notifications (a message with no `id`) get no reply and are never run, whatever they name: a command whose outcome nobody
can be told would be an edit from nowhere. Anything else is `-32601`, method not found. Resources, prompts, completions,
logging, progress, cancellation and subscriptions are not implemented, so they are not advertised, and the transport is
standard input and output only.

### Limits

Everything on the input is untrusted.

| What | Limit | Over it |
|---|---|---|
| A line | 16 MiB, not counting the line ending (`\r\n` or `\n`) | One `-32600` error (`request line longer than …`); the line is read through without being held, and the server goes on with the next. What was in it does not run, even if its start was a complete request. |
| Nesting of JSON | 128 levels (`serde_json`'s limit) | One `-32700` error (`recursion limit exceeded`); the server goes on. |
| A batch | 64 messages | One `-32600` error for the whole batch; none of its messages runs. An empty batch is the same. |
| `command_batch` | 256 steps | A tool error that names the limit ("`steps` holds 257 steps; a batch takes at most 256"), given before any step is looked at, so that a line of a million steps costs nothing beyond parsing it; none runs. The engine's `command.batch` limits the count as well. |

Batches (a JSON array of messages) are accepted for clients older than `2025-06-18`, which removed them; the reply is an
array of the replies there are.

**What a line of 16 MiB costs.** The limit is what the plan fixed, and a JSON value takes far more memory than its text: a
line of 16 MiB needs about 1.1 to 1.5 GiB to parse when it holds small objects, and about 2 GiB when the objects are
nested a hundred levels deep (measured again at on the release binary, as the peak resident memory of the server: 1,086 MiB for
one-key steps, 1,425 MiB for `{"a":0}` elements, 1,552 MiB for `{"":0}` and 2,059 MiB for `{"":{"":…0…}}` nested 124 deep, the
most of the shapes tried, with 128 levels the limit on nesting), so a client that wants to can make the server use that much memory, once at a time,
because one line is handled and freed before the next is read. That is accepted. What the tools do with a line adds
nothing to it: they move what they were given (`command_run`'s `params`, `command_batch`'s steps) and never copy it, and a
batch of more than 256 steps is refused before a command is built for any step or a request is sent. Before the tools stopped
copying, a line of 1.5 million one-key steps took 6,669 MiB, and a `command_run` with large `params` 7,053 MiB; an allocation that fails ends the process
and the session with it, so this is tested (`a_16_mib_line_costs_what_parsing_it_costs_and_no_more`, Linux only).

### `_meta`

The specification reserves `_meta` for metadata that clients and servers attach to their messages, and a client may send it:
in `2025-06-18`, `params` of any request is `{ _meta?: { progressToken?: …; [key: string]: unknown }; [key: string]: unknown }`
(*Basic protocol, General fields*, and the schema's `JSONRPCRequest`: <https://modelcontextprotocol.io/specification/2025-06-18/basic>,
<https://modelcontextprotocol.io/specification/2025-06-18/schema>), and a notification's `params` may hold it too. The
progress token a client wants notifications for travels there. The server accepts `_meta` in the `params` of every request it
answers (`initialize`, `ping`, `tools/list` and `tools/call`) and in a notification's, whatever its shape (an object, text, a
number, `null`, a deeply nested or a very large value), and does not look at it: it sends no progress notifications, which the
specification lets a receiver choose not to. The params `initialize` defines (`protocolVersion` and the rest) are read as they
always were. `_meta` is not a key of the message itself or of a tool's `arguments`: the specification puts it in `params`, so
there it is an unknown key like any other (`-32602`).

### What each kind of bad input gets

Each gets one reply and the server goes on, except that a message that gets none cannot be told anything.

| Input | Reply |
|---|---|
| Text that is not JSON, or not UTF-8 | `-32700` |
| JSON that is not an object (a number, `null`, a string) | `-32600` |
| No `"jsonrpc": "2.0"`, no text `method`, or an `id` that is not a string or a number (`null` included) | `-32600` |
| A key other than `jsonrpc`, `id`, `method` and `params` | `-32602`, naming the key |
| `params` that is not an object | `-32602` |
| An unknown tool, or a `tools/call` without a text `name` or with `arguments` that is not an object | `-32602` |
| A key of `tools/call` other than `name`, `arguments` and `_meta` | `-32602`, naming the key |
| A tool argument the tool does not take | `-32602`, naming the argument and listing the ones it takes |
| An unknown method | `-32601` |
| A bug in the server, caught | `-32603`, and the server goes on |

A key is refused, never ignored, so that a client that misspells `arguments` is told. The unknown keys that `-32602` is
for are the ones at the **top** of a message, of `tools/call`'s `params` and of a tool's `arguments`. The `params` that a
tool hands on to a command are the command's to check: a key it does not take comes back as an ordinary `isError` result
from the engine, with the nearest key it knows (`did you mean `name`?`).

Text a client sent that an error repeats (a tool name, a key, a method) is escaped first, and cut at 64 characters: a
control character, a line separator or a right-to-left override in it reaches the reply as `\u{1b}`, not as itself.

### Tool errors are results

An error in a tool's own work is the **result** of the call, with `isError: true`, so that the model reads it and corrects
itself: a command that fails or is not enabled, a parameter the command refuses, an argument of the wrong type, and a bug
the server caught in the tool (`internal error in …`, with a request to report it). A failed command changes nothing.
A protocol error is a JSON-RPC error. Replies hold no raw U+2028 or U+2029, which some line readers split a line at.

### A session

These are real lines, from the built binary. Keys come out in alphabetical order. The client's lines are marked `>`, the
server's `<`.

```
> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"file.new"}}}
< {"id":1,"jsonrpc":"2.0","result":{"content":[{"text":"{\"history\":{\"redo\":0,\"undo\":0},\"pages\":[{\"id\":1,\"name\":\"Page-1\"}],\"title\":\"Untitled\",\"version\":1}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"page.rename","params":{"id":99,"name":"x"}}}}
< {"id":2,"jsonrpc":"2.0","result":{"content":[{"text":"invalid parameters for `page.rename`: no page has id 99","type":"text"}],"isError":true}}
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"file.new","bogus":1}}}
< {"error":{"code":-32602,"message":"unknown argument `bogus` for `command_run`; it takes: id, params"},"id":3,"jsonrpc":"2.0"}
```

The second is a tool error (the command refused, `isError`), the third a protocol error (the call itself is malformed, so
nothing ran).

## The tools

A tool's text is compact JSON.

| Tool | Arguments | Result |
|---|---|---|
| `command_list` | `filter` (text in a command's id or label, any case), `enabled_only`, `brief` (leave out the parameter docs) | An array of `{id, label, menu, shortcut, params, enabled, disabled_reason?}`. |
| `command_run` | `id`, `params` (object) | The command's own result. |
| `command_batch` | `steps`: `[{id, params?}]`, at most 256; `label` (the undo step's name) | `{results: […]}`, in order. |
| `doc_inspect` | none | `{title, version, pages: [{id, name}], history: {undo, redo}}`, or `isError` when no document is open. |
| `ui_inspect` | none | The window's state (`ui.inspect`). Needs the app. |
| `ui_screenshot` | `path`: absolute, ending in `.png`, in a folder that exists | `{path, width, height}` once the next frame is written there. Needs the app. |

`command_batch` is the engine's `command.batch`: the steps are one undo step, all or nothing, and a batch whose steps leave the drawing as it was is no step (of the editing commands). A step that fails undoes the
rest and the error names it (`batch step 1 (`page.rename`) failed: …`, counting from 0). A batch needs a document, and cannot
hold `file.new`, `edit.undo`, `edit.redo` or another batch. Every step is checked before the first runs.

A command's parameters are described in prose, in the `params` of its listing; the registry does not give a schema.

**Reading a ShapeSheet**. `command_run` of `sheet.read {id, page?, sections?, from?, count?, all?}` (or with `page`, `master`, `style` or `document: true` for another sheet) reads the sections of a sheet in the window's order, each cell with its formula, its value in its unit, the text a window shows and where it comes from; `sheet.cell {id, cell}` reads one cell whole with the sheets it is looked for on, `sheet.sections {id}` says what Insert Section and Delete Section offer, and **`sheet.evaluate {id, formula, cell?}` tries a formula on the sheet and writes nothing**, so an agent can read first and test a formula before a later task lets it write one. The answers are the command's own JSON, as for every command (`command_list {filter: "sheet."}` lists them with their parameters); a sheet that is not there, a bad mix of sheet parameters and a cell that is no reference are `isError` results in words. A read gives at most 2000 lines, cuts a text at 4096 characters (`truncated`) and an answer at 8 MiB (`more`). See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".

**Setting ShapeSheet cells**. `command_run` of `sheet.set_cell {id, cell, formula, force?}` (or with `page`, `master`, `style` or `document: true` for another sheet) writes one cell as the window's formula bar will, and `sheet.set_cells {writes: [{cell, formula, force?}, ...]}` writes up to 4,096 in one undo step, all or nothing: `{"id":"sheet.set_cell","params":{"id":3,"cell":"Width","formula":"2.5 in"}}` as the arguments of `command_run` answers the cell as `sheet.cell` gives it after the write and, in `recalculated`, the cells that changed besides it (at most 256 are listed). An agent reads a cell first (`sheet.read`, `sheet.cell`) and tries a formula before it writes one (`sheet.evaluate`). A text that does not parse (with the kind of fault, its byte and words), a cell that is not there (the words say to add its row first), a guarded cell written without `force: true`, a shape on a locked layer and a call past the limits (65,536 bytes of formula, 4,096 writes, 4 MiB of text) are `isError` results in words and change nothing; a write that changes nothing is no undo step. See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".

**Rows and sections of a ShapeSheet**. `command_run` of `sheet.add_row {id, section, after?, before?, name?, type?}`, `sheet.delete_row {id, section, row}`, `sheet.add_section {id, section, variant?}` and `sheet.delete_section {id, section}` (or with `page`, `master`, `style` or `document: true` for another sheet) adds or deletes one row or section as one undo step: `{"id":"sheet.add_row","params":{"id":3,"section":"Prop","name":"Cost"}}` as the arguments of `command_run` answers the row (`row`, and `label` as a reference spells it, `Prop.Cost`) and the cells it holds, each at its default. An agent asks `sheet.sections` first (`can_add`, `can_delete`) and reads the rows' numbers with `sheet.read`. **An insert in the middle numbers the rows after it again and rewrites what names them**: `renumbered` says how many rows moved, and how many formulas and `Connect` rows of the page were written again with the new numbers (the bytes of each number are all that change), so an agent does not rewrite them; a formula that named a deleted row reads `#REF!`. A sheet that gets a section from its master or its style takes its own copy of the rows first when the insert is in the middle (`renumbered.copied` says how many rows), in the same undo step: a later edit of the master's rows no longer reaches that instance. A master's own rows and a style's own rows are numbered by those that read them, so an insert in the middle of them is refused (`add the row at the end`); a delete of an inherited row is a deleted row (`how: marked`). A Geometry section that holds an Ellipse or an Infinite Line row holds it alone, and such a row is added only to an empty section. A section that something else makes (the text's Character rows, the layers, the comments), a name that is taken or too long, a Shape Transform delete and a shape on a locked layer are `isError` results in words and change nothing. See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".


**The shape operations**. `command_run` of `shape.union`, `shape.combine`, `shape.fragment`, `shape.intersect`, `shape.subtract`, `shape.join`, `shape.trim`, `shape.offset {ids, distance}` and `shape.fit_curve {ids, tolerance, flags?}` works on the shapes named (the first named is the first shape) and answers `{page, ids, deleted, kept, changed, not_fitted}`: the shapes made, the shapes deleted, the shape that stayed (Subtract's), the shapes changed in place (Fit Curve's) and the shapes Fit Curve left as they were that hold something to fit. Offset's `distance` and Fit Curve's `tolerance` are in the cells' own units, which are inches on a page of the default scale. The refusals are results in words (`isError`) and change nothing, and each command is one undo step. **The dialogs are the window's**: `operations.offsetDialog` and `operations.fitCurveDialog` open them in the app and are not in a headless server's `command_list`; a program sends the number to `shape.offset` or `shape.fit_curve` as it does for every dialog's command ([Editing commands](editing-commands.md), "Offset and Fit Curve").

The snapping commands (`shape.move`, `shape.resize`, `shape.rotate`, `shape.drop`, `connector.add`, `connector.reconnect`, `snap.at`) take `snap`: `true`, or an object with `zoom` and, by command, `lock` (`shape.move`: `"x"` or `"y"`, the move keeps to that page axis), `nudge: {tick}` (`shape.move`: one press of an arrow key, `dx` and `dy` giving the direction, one of them 0, `tick` inches when nothing is ahead, else the ruler tick at the zoom), `view: [x0, y0, x1, y1]` (`shape.resize`: the window's visible part of the page in inches, so a width or height stops at that of a shape in it, every indexed shape when it is left out), `keep: "proportions"` (`shape.resize`: a corner keeps the shape's proportions, a side keeps the opposite side and the other axis grows about its middle) and `radius` (`shape.rotate`, with `angle`: how far the pointer is from the pin in inches, for how far another shape's angle pulls and for the rotation handle's step). A command takes only the words it has (`zoom` and `view` are every command's, `lock` and `nudge` `shape.move`'s, `keep` `shape.resize`'s, `radius` `shape.rotate`'s) and refuses the others in words. `snap.at` takes `edges` (an array of `left`, `bottom`, `right` and `top`), `keep`, `lock`, `nudge` (`{direction, tick}`), `view` and `rotate` (`{angle, radius}`) beside a place or a box, and tells what the matching command would do. Every `snap` result carries `marks`: lines with the shapes they run to, equal gaps with their lengths and chains, sizes and angles, each with its label, and `landed` (a resize that keeps proportions) and `angle` (a turn), and, for `shape.move`, `shape.place` and `snap.at`, `corner_boxes`: how many of the moving shapes lined up by the box round their corners where the box round their paths was meant (a turned shape lines up by the box round the paths it draws; one whose paths the call's budget could not pay for, or that draws none, keeps its corners). A hit on a line along no page axis (a side of a turned shape's own, a slanted guide) has a `normal` in degrees and no `axis`, and its mark has `axis` null; a resize of a turned or flipped shape tells `frame`, the box it comes to (`centre`, `width`, `height`, `angle`), and `snap.at` refuses `connect` beside a box, `edges`, `nudge`, `keep` and `rotate` in words. They are listed in `docs/editing-commands.md`, "Snap, grid and guides".

**The page's routing and a connector's route**. `page.routing` reads what the page holds of its routing, `page.set_routing` writes it as one undo step and `connector.path` reads a connector's corners (paper inches), its style and
its reroute setting ([Editing commands](editing-commands.md), "The page's routing settings"); an agent that wants to check a route reads it back after it has changed the page. These are real lines, as in "A session": the page takes the flowchart style
from left to right (`routed` is how many connectors that follow the page were routed again, none here), and a direction the network style does not take is a tool error, in words.

```
> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"page.set_routing","params":{"style":"flowchart","direction":"left_to_right"}}}}
< {"id":2,"jsonrpc":"2.0","result":{"content":[{"text":"{\"appearance\":\"straight\",\"between_shapes\":{\"x\":0.375,\"y\":0.375},\"connector_to_connector\":{\"x\":0.125,\"y\":0.125},\"connector_to_shape\":{\"x\":0.125,\"y\":0.125},\"direction\":\"left_to_right\",\"held\":{\"appearance\":false,\"between_shapes\":false,\"connector_to_connector\":false,\"connector_to_shape\":false,\"line_jumps\":false,\"overlap\":false,\"plow\":false,\"separate\":false,\"shape_size\":false,\"split\":false,\"style\":true},\"line_jumps\":{\"code\":1,\"dir_x\":0,\"dir_y\":0,\"factor_x\":0.6666666666666666,\"factor_y\":0.6666666666666666,\"style\":0},\"overlap\":\"by_style\",\"page\":1,\"plow\":false,\"routed\":0,\"separate\":\"unrelated\",\"shape_size\":{\"x\":0.25,\"y\":0.25},\"split\":false,\"style\":\"flowchart\",\"style_code\":6}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"page.set_routing","params":{"style":"network","direction":"left_to_right"}}}}
< {"id":3,"jsonrpc":"2.0","result":{"content":[{"text":"invalid parameters for `page.set_routing`: the style `network` takes no direction: leave `direction` out","type":"text"}],"isError":true}}
```

**Reverse Ends**. `command_run` of `connector.reverse_ends {id or ids, page?}` swaps the begin and the end of connectors and lines in one undo step (at most 4,096 shapes), each end staying glued to what it was glued to, the path walked the other way and the arrow
ending up at the other end ([Editing commands](editing-commands.md), "Reverse Ends"). The answer says what it did (`says`) and lists what it left alone (`skipped`, `locked`, `geometry_kept`); a call that reverses nothing is a tool error, in words. These are real lines:
a connector (shape 3, from a rectangle to an ellipse) is reversed, then the rectangle, which has no begin and end, is refused.

```
> {"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"connector.reverse_ends","params":{"id":3}}}}
< {"id":6,"jsonrpc":"2.0","result":{"content":[{"text":"{\"begin\":{\"id\":2,\"point\":1},\"end\":{\"id\":1,\"point\":3},\"id\":3,\"ids\":[3],\"page\":1,\"reversed\":1,\"says\":\"Reversed shape 3: its begin and end are swapped, with their glue, and the arrows stay on the ends of the path.\"}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"connector.reverse_ends","params":{"id":1}}}}
< {"id":7,"jsonrpc":"2.0","result":{"content":[{"text":"invalid parameters for `connector.reverse_ends`: shape 1 is not a connector or a line: it has no begin and end to swap","type":"text"}],"isError":true}}
```

**Connection points**. `command_run` of `shape.connection_edges {id or ids, page?, min_gap?}` reads the edges of a shape's outline and the connection points on each, and says what a doubling of each edge would make (`adds`, and `gap`, the smallest gap it would make) under a minimum distance between points (`min_gap`, paper inches, from 0.01 to 2, 3 mm when it is left out); it changes nothing ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)"). These are real lines, as in "A session", after `file.new` and a `shape.drop` of a diamond at (3, 2): the diamond's four points are on its corners, so on no edge (`other` lists them), a doubling of one of its sides would put points 0.45 inch apart, under the 0.5 inch asked for, so `adds` is 0 and `can_double` is false, and a `min_gap` outside its range is a tool error, in words.

```
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"shape.connection_edges","params":{"ids":[1],"min_gap":0.5}}}}
< {"id":3,"jsonrpc":"2.0","result":{"content":[{"text":"{\"min_gap\":0.5,\"page\":1,\"scale_note\":null,\"shapes\":[{\"can_double\":false,\"edges\":[{\"adds\":0,\"edge\":0,\"from\":{\"x\":3.0,\"y\":1.5},\"gap\":0.45069390943299864,\"kind\":\"straight\",\"length\":0.9013878188659973,\"path\":[{\"x\":3.0,\"y\":1.5},{\"x\":3.75,\"y\":2.0}],\"points\":[],\"rows\":[2,2],\"section\":\"Geometry1\",\"steps\":1,\"to\":{\"x\":3.75,\"y\":2.0}},{\"adds\":0,\"edge\":1,\"from\":{\"x\":3.75,\"y\":2.0},\"gap\":0.45069390943299864,\"kind\":\"straight\",\"length\":0.9013878188659973,\"path\":[{\"x\":3.75,\"y\":2.0},{\"x\":3.0,\"y\":2.5}],\"points\":[],\"rows\":[3,3],\"section\":\"Geometry1\",\"steps\":1,\"to\":{\"x\":3.0,\"y\":2.5}},{\"adds\":0,\"edge\":2,\"from\":{\"x\":3.0,\"y\":2.5},\"gap\":0.45069390943299864,\"kind\":\"straight\",\"length\":0.9013878188659973,\"path\":[{\"x\":3.0,\"y\":2.5},{\"x\":2.25,\"y\":2.0}],\"points\":[],\"rows\":[4,4],\"section\":\"Geometry1\",\"steps\":1,\"to\":{\"x\":2.25,\"y\":2.0}},{\"adds\":0,\"edge\":3,\"from\":{\"x\":2.25,\"y\":2.0},\"gap\":0.45069390943299864,\"kind\":\"straight\",\"length\":0.9013878188659973,\"path\":[{\"x\":2.25,\"y\":2.0},{\"x\":3.0,\"y\":1.5}],\"points\":[],\"rows\":[5,5],\"section\":\"Geometry1\",\"steps\":1,\"to\":{\"x\":3.0,\"y\":1.5}}],\"id\":1,\"other\":[0,1,2,3],\"points\":4,\"refused\":null,\"truncated\":false}],\"units_per_inch\":1.0}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"shape.connection_edges","params":{"id":1,"min_gap":5}}}}
< {"id":4,"jsonrpc":"2.0","result":{"content":[{"text":"invalid parameters for `shape.connection_edges`: `min_gap` must be a number of paper inches from 0.01 to 2","type":"text"}],"isError":true}}
```

**Doubling and halving connection points**. `command_run` of `shape.connection_points {id or ids, action, edge?, at?, reach?, page?, min_gap?}` doubles (a new point in the middle of every portion of the edge, nothing moved) or halves (the newest doubling's points, never an original) the connection points of the edges it is given, every edge of every shape named when it gives no `edge` and no `at`, as one undo step with every connector kept glued ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)"). These are real lines, as in "A session", after `file.new` and a `shape.drop` of a Rectangle at (3, 2): the first doubles every edge (the Rectangle's four points become twelve), the second halves its edge 0 (three points of it become one), and the third is a tool error in words, since `set_count` was dropped.

```
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"shape.connection_points","params":{"id":1,"action":"double"}}}}
< {"id":3,"jsonrpc":"2.0","result":{"content":[{"text":"{\"action\":\"double\",\"changed\":true,\"min_gap\":0.11811023622047245,\"page\":1,\"reglued\":[],\"renumbered\":0,\"scale_note\":null,\"shapes\":[{\"edges\":[{\"after\":3,\"before\":1,\"edge\":0,\"generation\":1,\"kind\":\"straight\"},{\"after\":3,\"before\":1,\"edge\":1,\"generation\":1,\"kind\":\"straight\"},{\"after\":3,\"before\":1,\"edge\":2,\"generation\":1,\"kind\":\"straight\"},{\"after\":3,\"before\":1,\"edge\":3,\"generation\":1,\"kind\":\"straight\"}],\"id\":1,\"points_after\":12,\"points_before\":4,\"skipped\":null,\"skipped_edges\":[]}]}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"shape.connection_points","params":{"id":1,"action":"halve","edge":0}}}}
< {"id":4,"jsonrpc":"2.0","result":{"content":[{"text":"{\"action\":\"halve\",\"changed\":true,\"min_gap\":0.11811023622047245,\"page\":1,\"reglued\":[],\"renumbered\":0,\"scale_note\":null,\"shapes\":[{\"edges\":[{\"after\":1,\"before\":3,\"edge\":0,\"generation\":1,\"kind\":\"straight\"}],\"id\":1,\"points_after\":10,\"points_before\":12,\"skipped\":null,\"skipped_edges\":[]}]}","type":"text"}],"isError":false}}
> {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"command_run","arguments":{"id":"shape.connection_points","params":{"id":1,"action":"set_count"}}}}
< {"id":5,"jsonrpc":"2.0","result":{"content":[{"text":"invalid parameters for `shape.connection_points`: set_count was dropped: double or halve","type":"text"}],"isError":true}}
```

## When the app does not answer

A call to the app can fail in ways that matter to what is done next, and the error says which:

| The error says | What it means |
|---|---|
| `nothing was sent` | The request did not leave: the app is not reachable, something else answers, or the request is over the app's 4 MiB line. Try again. |
| `nothing was run` | The app refused the line (too many connections, for one) and ran nothing. Try again. |
| `may have completed` | The request was, or may have been, sent and no usable answer came (a closed connection, the app not taking the request or answering within 90 seconds in all, a reply for another request, a reply over 16 MiB). Look at the document before trying again. |

A request has one budget of 90 seconds (the app gives up on a request after 60), for sending it and waiting for the reply
together. It is one budget on purpose: a program that takes the request a few kilobytes at a time, or answers a byte at a time,
makes every single read and write succeed, and would hold the server, which handles one request at a time, for as long as it
liked. A request of 4 MiB is the most the app reads.

## Logs

Standard output carries the protocol and nothing else. Log records go to standard error, and so does the line the server
prints when it starts, which names its backend. `RUST_LOG` sets the level (`off`, `error`, `warn`, `info`, `debug`,
`trace`; `warn` by default). No colour option or variable changes what the server writes: the protocol and these lines are plain, whatever `--color`, `NO_COLOR` or `CLICOLOR_FORCE` say
([`docs/cli.md`](cli.md), "Help, colour and the terminal").

## Not yet

`file.open` reads, and `file.save`, `file.save_as` and `file.export` (a picture of one page, the first page that is not a background when none is named, `.png`, `.svg` or `.pdf`, which never writes the open drawing's own file) and `file.print` (the PDF a printer would be given, a page for each sheet of paper; it reaches no printer) write, the paths the server may reach: **any path the user can, with no root given, and only the granted folders with one** ([Keeping files to the folders you grant](#keeping-files-to-the-folders-you-grant); gaps A2 and A5). What is refused, whatever the roots, is a link at the path of a save that does not lead to the drawing's own file, a name without a drawing extension, a folder, a device or a pipe (an export has its own extensions, `.png`, `.svg` and `.pdf`). The six tools will grow with the commands, which
`command_list` shows.

**The check after a save**. A `file.save_as` through a model writer (a VSDX type or an `.odg` written from the model, a `.diacraft` file written from a drawing of another format or made from a template file of one) reads the file back and compares it with the drawing, and its result has a `checked` field, `{done, classes: [{class, words, count}], as_values, reason}`, next to `carried`: each class that differs with the number of its items, `as_values` for the same content in the forms the format holds, and a formula that does not come back as a formula counted in `formulas`. `null` for a save into the drawing's own file, which is not checked in 0.2. The agent can say what differs to the person, and the drawing in the session is as it was (`docs/diacraft-format.md`, 7.7).

**The check before a save**. `command_run` of `file.check_compatibility {path, drop_macros, drop_objects, force}` asks what a `file.save_as` of that path would carry and lose, and answers `{writer, format, variant, carried, checked, bytes}` with no file written and nothing in the session changed (no undo step, and the drawing is not marked as changed): `writer` is `diacraft`, `vsdx_model`, `vsdx_splice`, `odg_model` or `odg_splice`, and `carried`, `checked` and `bytes` are the fields the save's own result holds (`checked` is `null` for a splice into the drawing's own format). With no `path` it asks about the drawing's own file, as `file.save` would save it. A save that would be refused is an `isError` result in the save's own words. `file.save_as` is unchanged and never asks: an agent that wants the answer first runs this command, which costs what the save costs less the disk.

**A `.diacraft` file** (DiaCraft's own format; `docs/diacraft-format.md`). `command_run` of `file.open` reads one by its bytes, and the file's own type decides, not its extension (`file.variant` is `diacraft`, `diacraft-stencil` or `diacraft-template`, and `file.format` the version it says); `file.save` writes it, and `file.save_as` takes the type from the extension and **makes the file written the drawing's own file** from then on. A drawing is saved as `.diacraft` from a new drawing, from a VSDX one and from one opened from an `.odg` (the file holds everything the `.odg` held); a `.diacraft` drawing is saved as a VSDX type from the drawing alone, is saved as an `.odg` or an `.otg` too, and a drawing opened from an `.odg` as a VSDX type, each from the drawing alone (what the file cannot say rides inside it, and the result's `carried` field says what; `drop_macros` and `drop_objects` leave the macro project and the objects out), a macro project is refused for a VSDX type that allows none, as an `isError` result whose words name `.vsdm` as the way out, with nothing written. **A new drawing is a `.diacraft` drawing**: `file.new` keeps its model and no file's bytes, so a `file.save_as` of it to a `.diacraft` name copies nothing and to a VSDX type writes the file from the drawing alone, and a drawing opened from a VSDX file is saved as VSDX by `file.save`, with nothing said of `.diacraft`. `file.status` answers `{dirty, has_file, format, variant, kind}` (`format` is `diacraft`, `vsdx` or `odg`; the last three are `null` for a drawing that has no file of a type yet). No new tool: the params docs and `command_list` carry it. **The trip through `.diacraft`** that the command line makes in one step (`diacraft-cli roundtrip FILE --out PATH --via diacraft`, `docs/cli.md`) is these commands in two sessions: `file.open` the file, `file.save_as` a `.diacraft`, then in **a new server session** (a second `mcp` process, so that nothing of the first one is kept) `file.open` that file and `file.save_as` the output in the format the file came in; for a VSDX package every step is made, and for an `.odg` the first `file.save_as` writes the `.diacraft` file and the last, the save back as an `.odg`, writes it from the drawing (the `.odg` model writer). The content check that judges the trip is the command line's `compare --content`, and no MCP tool.

**An OpenDocument drawing** (`.odg`). `file.open` reads one (the package's own type decides, and `file.open` says `variant: "odg"`), and `file.save` and
`file.save_as` write it, by splicing the edit into the file's own bytes; a `file.save_as` of it to an `.otg`, a `.diacraft` name or a VSDX type, and of a VSDX, `.diacraft` or new drawing to an `.odg` name, is written from the drawing by the model writer of the target (above). A change the `.odg` writer does not write is a
refused save: the tool answers `isError` with the writer's words (for example `... the drawing could not be saved: saving does not yet write this change: a dashed line (...)`), nothing is
written and the drawing in the session is as it was, so the agent can take the change back with `edit.undo` and save (the words end with "Undo the change (edit.undo) to save", except a refusal for memory, which has its own words and its own way out, one for a shape an undo put back, whose words say to redo with `edit.redo`, and one for a group with no name that holds data, whose words name the group and say to name it, to put its data on a shape beside it or to use Save As for a `.diacraft` file). The answers of `connector.add` and `connector.style` carry no `warnings` list for a curved connector (the save writes it as cubics), and the answer of a save says in `warnings` what a new shape held that the file does not (the text formatting a style sheet gives the text, and the link to the Theme style, each by its kind: the rest of what a new shape holds is carried). A drawing template (`.otg`) opens as a new drawing made from it (`file.new_from_template: true`), and `edit_template: true` opens the template itself. **The open says what the drawing does not hold**: the first of `file.warnings` is then `What DiaCraft does not hold of this file: ...`, and a line for each class with its count follows; a save into the file keeps it, and a copy saved as another file is without it.

**A flat OpenDocument drawing** (`.fodg`, one XML document). `file.open` reads one by what the file is and not by its name, and says `file.form: "flat"` (`"package"` for an `.odg`) and, first among `file.warnings`, what Save does with it. **`file.save` writes it back as a flat file, in place**, whatever the file is called: unedited it is the file's own bytes (`edited: false`), an edit is spliced into the one document, and the answer says `form: "flat"`; a change the flat writer does not write (a master a stencil shape brings, a page added) is an `isError` result with the writer's words, and nothing is written. **`file.save_as` is available for it** (it was listed `enabled: false` with a reason and left out of `enabled_only`): it writes the drawing from the model as a `.diacraft` file, an `.odg` or `.otg` package or a VSDX type, and leaves the flat file as it was; a `file.save_as` to a `.fodg`, for this drawing or any other, is an `isError` result that says Save As does not offer it and points to the `.odg`. The agent can export a page with `file.export`.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-26 | minor | "The tools" has an example of `connector.reverse_ends` through `command_run` (real lines: a connector reversed, a box refused in words) |
| 2026-11-26 | minor | `shape.connection_points`, which doubles and halves the connection points of a shape's edges as one undo step, with real lines of a doubling, a halving and the refusal of `set_count` ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)") |
| 2026-11-26 | minor | "How a path is judged" says that where the system does not say which file a handle is, the look at it is the file's identity on Windows too (its volume serial number and file index), no longer a size and a modified time, so another file of that size and time put in the place is refused, and "What it keeps to" says a read's race is closed on Linux and narrowed elsewhere, to a folder on the way swapped for a link and back |
| 2026-11-26 | minor | "How a path is judged" says a `..` below a missing folder is refused on Windows too, that on Linux the system's answer for the handle a read opened is final (and on Windows the look is weaker), and that a write is made at its folder's real path and its own name, a link standing at the name refused |
| 2026-11-26 | minor | The refusal of an `.odg` save for a group with no name that holds data names the group and does not say to undo |
| 2026-11-26 | minor | A paragraph on the shape operations through `command_run` (Offset's distance and Fit Curve's tolerance in the cells' own units, the answer `{page, ids, deleted, kept, changed, not_fitted}`; the dialogs are the window's) |
| 2026-11-27 | minor | `command_batch` whose steps leave the drawing as it was is no undo step (of the editing commands) |
| 2026-11-26 | minor | "Keeping files to the folders you grant" says the folders are the flags' and the variables' alone: the server does not ask the client for its roots (`roots/list`) and does not read the roots a client announces |
| 2026-11-26 | minor | Round 2: the sentence on what `file.export` writes keeps both the first page that is not a background and the folders of the automation roots |
| 2026-11-26 | minor | Follow-up: a command that is not given a page, `file.export` among them, works on the first page that is not a background (of the editing commands) |
| 2026-11-26 | minor | `shape.connection_edges`, a query of the edges of a shape's outline, the connection points on each and what a doubling of each would make under a minimum distance between points ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)") |
| 2026-11-26 | minor | "Keeping files to the folders you grant": `--automation-read-root` and `--automation-write-root` (and the two variables) keep a session of this process to folders, how a path is judged, the words of a refusal, that `initialize` names the folders, that `--connect` with a root flag is an error and the default mode stops when the app answers, and what is not kept; the bundle's paragraph and "Not yet" say so |
| 2026-11-26 | minor | `command_run` of `sheet.set_cell` and `sheet.set_cells` writes a cell of any sheet (one undo step, all or nothing), with an example |
| 2026-11-26 | minor | `command_run` of `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section` adds and deletes the rows and sections of any sheet (one undo step each; the rows after an insert numbered again and the formulas that name them written again), with an example |
| 2026-11-26 | minor | Follow-up: an insert in the middle of a section the sheet inherits takes a copy of the rows first (`renumbered.copied`), and a section that holds an Ellipse or an Infinite Line row holds it alone |
| 2026-11-25 | minor | `command_run` of `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate` reads a sheet and tries a formula on it, with nothing written |
| 2026-11-25 | minor | "The tools" has an example of the page's routing settings (`page.set_routing`) and a connector's route (`connector.path`) run through `command_run` |
| 2026-11-25 | minor | `command_run` of `file.check_compatibility` answers what a save would carry and lose, with no file written |
| 2026-11-25 | minor | "Installing it from a bundle or a directory": the six `.mcpb` bundles of a release (one for each system and CPU), what a bundle holds and starts, the files being unsigned, the registry's entry `app.diacraft/diacraft` and what was tried |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | The trip of an `.odg` through `.diacraft` writes the last leg from the drawing (it said an `isError` until a later change), a drawing of another type is saved as an `.odg` and an `.odg` as another type (it said never), an `.otg` opens (it said not yet), and the open of an `.odg` names what the drawing does not hold |
| 2026-11-23 | minor | `checked` covers a drawing made from a template file of another format. |
| 2026-11-22 | minor | The result of `file.save_as` has `checked`: the file read back and compared with the drawing after a save through a model writer. |
| 2026-11-22 | minor | `command_list` lists `file.save_as` enabled for a drawing opened from a flat file and keeps it in `enabled_only`, and a `command_run` of it writes the drawing as a `.diacraft` file, an `.odg` or a VSDX type; a `.fodg` is still refused |
| 2026-11-22 | minor | `file.save_as` saves across the VSDX and OpenDocument formats, takes `drop_macros` and `drop_objects`, and its result has `carried`. |
| 2026-11-21 | minor | The `.odg` save's `warnings` say two kinds of loss, and `connector.add` and `connector.style` carry none for a curved connector |
| 2026-11-21 | minor | The `.diacraft` paragraph says a new drawing keeps no file's bytes, so `command_run` of `file.save_as` writes `.diacraft` from it with nothing copied and a VSDX type from the drawing alone, and a drawing opened from a VSDX file is saved as VSDX, silently |
| 2026-11-21 | minor | The paragraph on the trip through `.diacraft` says an `.odg` is saved as `.diacraft` and refused at the save back as an `.odg`, and not at the first save (it said the first `file.save_as` of an `.odg` was refused, which lifted) |
| 2026-11-21 | minor | `command_run` of `file.save_as` of a drawing opened from an `.odg` writes a `.diacraft` file; a save across to a VSDX type is still an `isError` result in words |
| 2026-11-21 | minor | The `.diacraft` paragraph says the trip through `.diacraft` is `file.open`, `file.save_as`, and in a new server session `file.open` and `file.save_as`, which the command line's `roundtrip --via diacraft` makes in one step, and that its content check, `compare --content`, is no MCP tool; no new tool |
| 2026-11-21 | minor | The `.diacraft` paragraph says a `.diacraft` drawing is saved as a VSDX type, from the drawing alone, and is not yet saved as an `.odg` |
| 2026-11-21 | minor | `command_run` of `file.open` reads a `.diacraft` file by its bytes, `file.save` and `file.save_as` write one and switch the drawing's own file, a save across the formats that is not written yet is an `isError` result that names the way out, and `file.status` answers `{dirty, has_file, format, variant, kind}`; no new tool |
| 2026-11-20 | minor | The status line's date is that of the newest row, 2026-11-20 (it said 2026-11-19) |
| 2026-11-20 | minor | A hit on a line along no axis has a `normal` and its mark `axis` null, a resize of a turned or flipped shape tells `frame`, `corner_boxes` is also told by `shape.place` and `snap.at`, and `snap.at` refuses `connect` beside a box, `edges`, `nudge`, `keep` and `rotate` |
| 2026-11-19 | minor | A command takes only the words of `snap` it has, and `snap.at` takes `edges`, `keep`, `lock`, `nudge`, `view` and `rotate` |
| 2026-11-19 | minor | Third part: the `snap` result of `shape.move` carries `corner_boxes`, how many moving shapes lined up by the box round their corners |
| 2026-11-19 | minor | `command_list` lists `file.save_as` with `enabled: false` and the reason for a drawing opened from a flat file, `enabled_only` leaves it out, and a `command_run` of it is an `isError` result that says why; `file.open`'s `warnings` are at most 20 with the notes of the open itself first and last |
| 2026-11-19 | minor | "The tools" names the words of `snap` (`lock`, `nudge`, `view`, `keep`, `radius`) and `marks` |
| 2026-11-18 | minor | `file.open` reads a flat OpenDocument drawing (`.fodg`) by what the file is and says `file.form: "flat"`; `file.save` writes it back as a flat file in place, and a `file.save_as` of it, and a Save As of another drawing to a `.fodg`, are `isError` results with words and nothing written |
| 2026-11-09 | minor | `command_run` of `file.open`, `file.save` and `file.save_as` reaches an OpenDocument drawing (`.odg`), and a change its writer does not write is an `isError` result whose text is the writer's own words, with nothing written |
| 2026-11-01 | minor | `file.export` writes a picture of a page, and the page on the automation roots says so |
| 2026-11-01 | minor | Saving exists; no granted folders (gaps A2, A5) |
| 2026-10-31 | minor | Final review: a 16 MiB line costs about 1.1 to 1.5 GiB to parse when it holds small objects and up to about 2 GiB when the objects are nested deeply (the page said about 1.1 GiB), measured on the release binary; the line of one-key steps that took 6,669 MiB before the tools stopped copying held 1.5 million steps, not 1.4 |
| 2026-10-31 | minor | What a 16 MiB line costs (and that the tools add nothing to it), one budget for sending a request and waiting for its reply, `_meta` in the params of every request (what the specification says), a reconnect is logged and what to do after one; the status line said three backends, there are two |
| 2026-10-31 | minor | Created: the six tools, the headless and app backends and the identity check, the limits, and what each kind of bad input gets; run with a Python client against the built binary |
