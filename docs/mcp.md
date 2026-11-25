# The MCP server

`diacraft-cli mcp` makes DiaCraft's commands available to an agent as [Model Context Protocol](https://modelcontextprotocol.io)
tools, over standard input and output. It is a different way to ask for what the menus and the
[control channel](control-protocol.md) ask for: every tool ends in the same command registry, so a command does the same
thing however it was asked for. The reference app has not been inspected, and nothing here claims what the reference app does.

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

## Installing it from a bundle or a directory

A release holds six **MCP bundles** (`.mcpb` files, the [MCP Bundle format](https://github.com/modelcontextprotocol/mcpb)), one for each system and CPU the downloads are built for:
`diacraft-mcp-<version>-linux-x86_64.mcpb`, `-linux-aarch64`, `-windows-x64`, `-windows-arm64`, `-macos-arm64` and `-macos-x86_64`. The format names the system a bundle is for and has no word
for a CPU, so each CPU has a file of its own: take the one that says yours. An app that installs MCP servers takes the file (open it with the app), and a directory of MCP servers lists it;
either way the person writes no configuration, and the app starts the server as the bundle's manifest says.

A bundle is a zip. It holds `manifest.json` (format 0.3: the name `diacraft`, the version, a `binary` server whose command is `${__dirname}/server/diacraft-cli` with the argument `mcp`, the one system it
is for, the six tools below, the licence `MIT OR Apache-2.0`, the home page and the repository), `icon.png`, the program `server/diacraft-cli` (`.exe` on Windows) and the five licence files that every
download holds. It holds the command line and nothing else: not the window. The server it starts is `diacraft-cli mcp`, in the default mode (above): a session of its own with no document, or the
running app's window if the app was started with `--control` and answers at `127.0.0.1:7997`. Install the app from the site to see a drawing in a window.

- **What an agent given the bundle can do.** Everything this page says the server can do: `file.open`, `file.save` and the rest read and write any path the server may reach (gaps A2 and A5). Give a
  bundle only to an agent that you trust with your files. The server connects to nothing but the DiaCraft app on the same computer.
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
| `initialize` | The revision, `{"tools": {"listChanged": false}}` as the only capability, `serverInfo` (`diacraft`) and short instructions that name the backend. |
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

`command_batch` is the engine's `command.batch`: the steps are one undo step, all or nothing. A step that fails undoes the
rest and the error names it (`batch step 1 (`page.rename`) failed: …`, counting from 0). A batch needs a document, and cannot
hold `file.new`, `edit.undo`, `edit.redo` or another batch. Every step is checked before the first runs.

A command's parameters are described in prose, in the `params` of its listing; the registry does not give a schema.

The snapping commands (`shape.move`, `shape.resize`, `shape.rotate`, `shape.drop`, `connector.add`, `connector.reconnect`, `snap.at`) take `snap`: `true`, or an object with `zoom` and, by command, `lock` (`shape.move`: `"x"` or `"y"`, the move keeps to that page axis), `nudge: {tick}` (`shape.move`: one press of an arrow key, `dx` and `dy` giving the direction, one of them 0, `tick` inches when nothing is ahead, else the ruler tick at the zoom), `view: [x0, y0, x1, y1]` (`shape.resize`: the window's visible part of the page in inches, so a width or height stops at that of a shape in it, every indexed shape when it is left out), `keep: "proportions"` (`shape.resize`: a corner keeps the shape's proportions, a side keeps the opposite side and the other axis grows about its middle) and `radius` (`shape.rotate`, with `angle`: how far the pointer is from the pin in inches, for how far another shape's angle pulls and for the rotation handle's step). A command takes only the words it has (`zoom` and `view` are every command's, `lock` and `nudge` `shape.move`'s, `keep` `shape.resize`'s, `radius` `shape.rotate`'s) and refuses the others in words. `snap.at` takes `edges` (an array of `left`, `bottom`, `right` and `top`), `keep`, `lock`, `nudge` (`{direction, tick}`), `view` and `rotate` (`{angle, radius}`) beside a place or a box, and tells what the matching command would do. Every `snap` result carries `marks`: lines with the shapes they run to, equal gaps with their lengths and chains, sizes and angles, each with its label, and `landed` (a resize that keeps proportions) and `angle` (a turn), and, for `shape.move`, `shape.place` and `snap.at`, `corner_boxes`: how many of the moving shapes lined up by the box round their corners where the box round their paths was meant (a turned shape lines up by the box round the paths it draws; one whose paths the call's budget could not pay for, or that draws none, keeps its corners). A hit on a line along no page axis (a side of a turned shape's own, a slanted guide) has a `normal` in degrees and no `axis`, and its mark has `axis` null; a resize of a turned or flipped shape tells `frame`, the box it comes to (`centre`, `width`, `height`, `angle`), and `snap.at` refuses `connect` beside a box, `edges`, `nudge`, `keep` and `rotate` in words. They are listed in `docs/editing-commands.md`, "Snap, grid and guides".

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
`trace`; `warn` by default).

## Not yet

`file.open` reads, and `file.save`, `file.save_as` and `file.export` (a picture of one page, `.png`, `.svg` or `.pdf`, which never writes the open drawing's own file) and `file.print` (the PDF a printer would be given, a page for each sheet of paper; it reaches no printer) write, any path the server may reach: the automation roots
(`--automation-read-root`, `--automation-write-root`) are not built, so an agent is not kept to granted folders (gaps A2 and
A5). What is refused is a link at the path of a save that does not lead to the drawing's own file, a name without a drawing
extension, a folder, a device or a pipe (an export has its own extensions, `.png`, `.svg` and `.pdf`). The six tools will grow with the commands, which
`command_list` shows.

**The check after a save**. A `file.save_as` through a model writer (a VSDX type or an `.odg` written from the model, a `.diacraft` file written from a drawing of another format or made from a template file of one) reads the file back and compares it with the drawing, and its result has a `checked` field, `{done, classes: [{class, words, count}], as_values, reason}`, next to `carried`: each class that differs with the number of its items, `as_values` for the same content in the forms the format holds, and a formula that does not come back as a formula counted in `formulas`. `null` for a save into the drawing's own file, which is not checked in 0.2. The agent can say what differs to the person, and the drawing in the session is as it was (`docs/diacraft-format.md`, 7.7).

**A `.diacraft` file** (DiaCraft's own format; `docs/diacraft-format.md`). `command_run` of `file.open` reads one by its bytes, and the file's own type decides, not its extension (`file.variant` is `diacraft`, `diacraft-stencil` or `diacraft-template`, and `file.format` the version it says); `file.save` writes it, and `file.save_as` takes the type from the extension and **makes the file written the drawing's own file** from then on. A drawing is saved as `.diacraft` from a new drawing, from a VSDX one and from one opened from an `.odg` (the file holds everything the `.odg` held); a `.diacraft` drawing is saved as a VSDX type from the drawing alone, is saved as an `.odg` or an `.otg` too, and a drawing opened from an `.odg` as a VSDX type, each from the drawing alone (what the file cannot say rides inside it, and the result's `carried` field says what; `drop_macros` and `drop_objects` leave the macro project and the objects out), a macro project is refused for a VSDX type that allows none, as an `isError` result whose words name `.vsdm` as the way out, with nothing written. **A new drawing is a `.diacraft` drawing**: `file.new` keeps its model and no file's bytes, so a `file.save_as` of it to a `.diacraft` name copies nothing and to a VSDX type writes the file from the drawing alone, and a drawing opened from a VSDX file is saved as VSDX by `file.save`, with nothing said of `.diacraft`. `file.status` answers `{dirty, has_file, format, variant, kind}` (`format` is `diacraft`, `vsdx` or `odg`; the last three are `null` for a drawing that has no file of a type yet). No new tool: the params docs and `command_list` carry it. **The trip through `.diacraft`** that the command line makes in one step (`diacraft-cli roundtrip FILE --out PATH --via diacraft`, `docs/cli.md`) is these commands in two sessions: `file.open` the file, `file.save_as` a `.diacraft`, then in **a new server session** (a second `mcp` process, so that nothing of the first one is kept) `file.open` that file and `file.save_as` the output in the format the file came in; for a VSDX package every step is made, and for an `.odg` the first `file.save_as` writes the `.diacraft` file and the last, the save back as an `.odg`, writes it from the drawing (the `.odg` model writer). The content check that judges the trip is the command line's `compare --content`, and no MCP tool.

**An OpenDocument drawing** (`.odg`). `file.open` reads one (the package's own type decides, and `file.open` says `variant: "odg"`), and `file.save` and
`file.save_as` write it, by splicing the edit into the file's own bytes; a `file.save_as` of it to an `.otg`, a `.diacraft` name or a VSDX type, and of a VSDX, `.diacraft` or new drawing to an `.odg` name, is written from the drawing by the model writer of the target (above). A change the `.odg` writer does not write is a
refused save: the tool answers `isError` with the writer's words (for example `... the drawing could not be saved: saving does not yet write this change: a dashed line (...)`), nothing is
written and the drawing in the session is as it was, so the agent can take the change back with `edit.undo` and save (the words end with "Undo the change (edit.undo) to save", except a refusal for memory, which has its own words and its own way out, and one for a shape an undo put back, whose words say to redo with `edit.redo`). The answers of `connector.add` and `connector.style` carry no `warnings` list for a curved connector (the save writes it as cubics), and the answer of a save says in `warnings` what a new shape held that the file does not (the text formatting a style sheet gives the text, and the link to the Theme style, each by its kind: the rest of what a new shape holds is carried). A drawing template (`.otg`) opens as a new drawing made from it (`file.new_from_template: true`), and `edit_template: true` opens the template itself. **The open says what the drawing does not hold**: the first of `file.warnings` is then `What DiaCraft does not hold of this file: ...`, and a line for each class with its count follows; a save into the file keeps it, and a copy saved as another file is without it.

**A flat OpenDocument drawing** (`.fodg`, one XML document). `file.open` reads one by what the file is and not by its name, and says `file.form: "flat"` (`"package"` for an `.odg`) and, first among `file.warnings`, what Save does with it. **`file.save` writes it back as a flat file, in place**, whatever the file is called: unedited it is the file's own bytes (`edited: false`), an edit is spliced into the one document, and the answer says `form: "flat"`; a change the flat writer does not write (a master a stencil shape brings, a page added) is an `isError` result with the writer's words, and nothing is written. **`file.save_as` is available for it** (it was listed `enabled: false` with a reason and left out of `enabled_only`): it writes the drawing from the model as a `.diacraft` file, an `.odg` or `.otg` package or a VSDX type, and leaves the flat file as it was; a `file.save_as` to a `.fodg`, for this drawing or any other, is an `isError` result that says Save As does not offer it and points to the `.odg`. The agent can export a page with `file.export`.

## Revision history

| Date | Change | Summary |
|---|---|---|
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
