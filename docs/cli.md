# The command line

`diacraft-cli` runs DiaCraft's commands without a window, for scripts and for agents. It reaches the same registry of
commands the menus and the [control channel](control-protocol.md) do, so a command does the same thing wherever it is
asked for. The reference app has never been taken apart: what the commands do is DiaCraft's own, and is not a claim about the reference
app.

```
diacraft-cli mcp [--headless | --connect ADDR] [--automation-read-root DIR]... [--automation-write-root DIR]... [--bundled-fonts]
diacraft-cli run [--cmd ID [--params JSON] | --commands FILE|-]... [--bundled-fonts]
diacraft-cli commands
diacraft-cli oracle FILE... [--json] [--source NAME]
diacraft-cli oracle --aggregate REPORTS... [--cap N]
diacraft-cli export FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
diacraft-cli convert FILE --out PATH [--drop-macros] [--drop-objects] [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
diacraft-cli roundtrip FILE --out PATH [--move DX,DY] [--bundled-fonts]
diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move|retext|delete|add | --move DX,DY] [--bundled-fonts]
diacraft-cli compare FIRST SECOND
diacraft-cli compare --content FIRST SECOND
diacraft-cli completions SHELL [--program diacraft|diacraft-cli]
diacraft-cli man [--list | PAGE]
diacraft-cli --color auto|always|never COMMAND ...
diacraft-cli --version
diacraft-cli --help
```

Results go to standard output and nothing else does: messages for people, and the log, go to standard error.

| Exit status | Meaning |
|---|---|
| 0 | It did what was asked. A reader that closed standard output early (`diacraft-cli run ... \| head -1`) is not a failure. |
| 1 | A step failed, a file could not be read, or a connection could not be made. Standard error says which. |
| 2 | A mistake in the command line: an option that does not exist, a value left out, an option given twice. Nothing ran. |

`--version` and `--help` win over everything else on the line, a mistake in it included, as long as they come before a `--` (after it they are file names). With no command at all the usage
is printed on standard error and the status is 2. The help is coloured and wrapped at a terminal, and plain everywhere else
([Help, colour and the terminal](#help-colour-and-the-terminal)); `completions` and `man` write the shell scripts and the manual pages ([below](#shell-completions)).

## Help, colour and the terminal

`diacraft-cli --help` prints one page, and `--help` anywhere on a line prints that same page (after a command too: `diacraft-cli export --help`). **When standard output is not a terminal** (a pipe, a
file, a script) the page is plain text, the same bytes everywhere, with the line breaks of the table of its words (`apps/diacraft-cli/src/help/text.rs`). **At a terminal** it is the same words,
wrapped to the terminal's width (kept between 40 and 120 columns; each paragraph and each option's description keeps its indent) and painted: headings in bold green, the names a person types
(commands, options, and what stands in backticks) in bold cyan, the places to fill in (`FILE`, `ADDR`, `<command>`) in cyan. Stripping the colour codes from the painted page gives the plain one.

**Whether to colour**, by `--color auto|always|never` (an option of every command, before or after it, given once; `auto` is the default) and the environment, in the order they are asked:

| Rule | Result |
|---|---|
| `--color always` | colour, whatever the environment says |
| `--color never` | plain, whatever the environment says |
| (`auto`) `NO_COLOR` set and not empty | plain |
| `CLICOLOR_FORCE` set, not empty and not `0` | colour, even where the output is not a terminal |
| `CLICOLOR=0` | plain |
| the stream is a terminal and `TERM` is neither empty nor `dumb` (a Windows console has no `TERM` and needs none) | colour |
| anything else (a pipe, a file, a dumb terminal) | plain |

Each stream is judged on its own: the page by standard output, the words of a failure by standard error, so a person who sends the JSON to a file still sees a failure in colour. A terminal's width is
read only for standard output, and only when it is a terminal: a pipe is never wrapped, so `diacraft-cli --help | grep ...` finds the same lines wherever it runs.

```
$ diacraft-cli --help | less -R                  # plain: a pipe
$ diacraft-cli --color always --help | less -R   # painted, for a pager that shows it
$ NO_COLOR=1 diacraft-cli --help                 # plain, at a terminal
$ CLICOLOR_FORCE=1 diacraft-cli --help | less -R # painted, though a pipe
```

**Colour is for people and never for programs.** It is in two places: the help page, and the words that tell a person that a command line is a mistake or that a step failed (`diacraft-cli:` in bold red,
the words after it plain, on standard error). It is never in what a program reads: the JSON lines of `run`, `commands`, `export` and the rest, the MCP stream, the exit statuses, and the notes the MCP
server writes on standard error (`diacraft-cli: MCP server on stdio, ...`) are the same bytes with every switch on, which `apps/diacraft-cli/tests/it/ux.rs` runs. Every escape is taken off the stream
that is plain, so even a message that held one could never put one on a terminal. On Windows a console is put in the mode that understands the codes (an old console that has none gets the same colours
through its own calls); a stream that is not a console gets no codes.

**The words of the options.** Every option stands where it likes, and a value is the next word whatever it begins with (`--out --bundled-fonts` names a file called `--bundled-fonts`). An option the command
does not have, a value left out and an option given twice are mistakes (status 2, nothing ran), in the words `docs/cli.md` has always had (`unknown option `--x` for `export`; see --help`, `--out was given
twice`, `--out needs a value`). `--` ends the options for every command (what follows is a file, `-odd.vsdx`, `--help` and `--version` included, so that `export --out p.svg -- --help` reads a drawing named `--help`; before it, a word with one dash is an option the command does not have,
as in the app: it was a file name before), and a command line is read by clap (`apps/diacraft-cli/src/cli.rs` describes it, once, for the parser, the completions and the manual pages).

## Shell completions

`diacraft-cli completions SHELL [--program diacraft|diacraft-cli]` writes the completion script of a shell on standard output, and nothing else: SHELL is `bash`, `zsh`, `fish`, `powershell` or
`elvish`. With `--program diacraft` the script is the app's, `diacraft`, which has options of its own (`--control PORT`, `--automation-read-root FOLDER` and `--automation-write-root FOLDER`, `--reset-graphics`, `--version`, `--help`, and the drawing to open). A shell or a program that is not
named is a mistake (status 2). The script is made by the program that runs, so it lists that build's commands.

What each script completes:

| | bash, zsh, fish, PowerShell | Elvish |
|---|---|---|
| The commands and the options of each | yes | yes |
| The values of an option that takes a few: `--text` (`text`, `outlines`), `--via`, `--edit`, `--color`, `--program`, the shell of `completions`, the page of `man`, the usual pixel counts of `--dpi` | yes (PowerShell's are added after what clap_complete writes, which has none) | no (clap_complete writes none) |
| `run --cmd`: every command id of the registry, the list `commands` prints, with its label in zsh, fish and PowerShell | yes | no |
| Files **by their type**: a drawing where a drawing is read (the types `file.open` reads: `.diacraft`, `.diacraft-stencil`, `.diacraft-template`, the six VSDX types, `.odg`, `.otg`, `.fodg`, in any case of the letters), a picture where `export` writes one (`.png`, `.svg`, `.pdf`), the types `convert` writes after `--out`, the reports `oracle --aggregate` reads, and always the folders, so that a path can be walked (the folder of `--automation-read-root` and `--automation-write-root`, in `mcp` and in the app's script, is completed as a folder and no other path); a steps file (`run --commands`) completes every file | yes (bash: the word after the command, and the word after `--out`; PowerShell completes a path as it does for any program) | the shell completes a path as it does for any program |

Where each shell looks:

| Shell | For you | For every user |
|---|---|---|
| bash | `diacraft-cli completions bash > ~/.local/share/bash-completion/completions/diacraft-cli` (the bash-completion package loads it), or `source <(diacraft-cli completions bash)` in `~/.bashrc` | `/usr/share/bash-completion/completions/diacraft-cli` |
| zsh | `diacraft-cli completions zsh > ~/.zfunc/_diacraft-cli`, and `fpath=(~/.zfunc $fpath)` before `compinit` in `~/.zshrc` | `/usr/share/zsh/vendor-completions/_diacraft-cli` |
| fish | `diacraft-cli completions fish > ~/.config/fish/completions/diacraft-cli.fish` | `/usr/share/fish/vendor_completions.d/diacraft-cli.fish` |
| PowerShell | `diacraft-cli completions powershell \| Out-String \| Invoke-Expression` in `$PROFILE` (`notepad $PROFILE`) | |
| Elvish | `eval (diacraft-cli completions elvish \| slurp)` in `rc.elv` | |

For the app's own options add `--program diacraft` and name the file `diacraft` (`_diacraft`, `diacraft.fish`).

**The packages.** The `.deb` installs both programs' scripts for bash, zsh and fish at the paths of the last column, and the manual pages (below) in `/usr/share/man/man1`; it writes them with the program it
packages, so a package for the other CPU than the machine's is made without them and says so. **The `.rpm` does the same**, with zsh's in `/usr/share/zsh/site-functions`, Fedora's folder, and not in Debian's `vendor-completions`. The tarball, the Windows zip and the macOS app install nothing: their `README.txt` shows the commands above.
The AppImage writes them through its command line (`./diacraft-<version>-linux-<cpu>.AppImage --cli completions bash`). The `.msi` adds none: `diacraft-cli completions powershell` is on the `PATH` it sets.
`packaging/smoke.sh` makes each script, parses it with the shells that are there (`bash -n`, `zsh -n`, `fish --no-execute`, PowerShell's parser) and, run on an installed `.deb` or `.rpm`, looks for the files (zsh's in either folder). The tests ask bash, fish and PowerShell what they would complete (`apps/diacraft-cli/tests/it/ux.rs`); zsh and Elvish are not asked, and their scripts are read.

clap_complete is held to 4.5: its 4.6 breaks bash completion of the commands of a program whose name has a hyphen (clap-rs/clap#6421), and a test asks bash itself to complete a line.

## Manual pages

`diacraft-cli man [--list | PAGE]` writes a manual page in the format the `man` program reads, on standard output. The pages are `diacraft-cli` (the program and its commands), one for each command
(`diacraft-cli-run`, `diacraft-cli-export`, ... as `git`'s are named) and `diacraft` (the app); `--list` names them, the program's first, one on a line. Read one with `diacraft-cli man diacraft-cli-run |
man -l -`. A page says what the help page says (the sentences are the table's, by key), so a change of the table changes both. The `.deb` and the `.rpm` install them gzipped in `/usr/share/man/man1`.

## Fonts

`run`, `export` and `roundtrip`, `convert` to a picture, and `mcp` when its session is the process's own, read the machine's font folders **once, at the start and before the
first command**, and the faces they find, with the bundled ones, are the faces the engine measures text in and the faces an export draws it in.
`mcp --connect ADDR` reads nothing (the session is the running app's and has the app's faces), and `mcp` in the default mode reads them only
once it has chosen a headless session, not while it is still looking for the app. They are one set: a text block that grows
with its text (`TEXTWIDTH` and `TEXTHEIGHT` in its cells) is sized, when a command changes what it reads, in the faces `export` draws it in, as the
window does. `--bundled-fonts` reads nothing from the machine and uses the bundled faces alone, which are the same everywhere: a run, and the
picture it exports, can be compared between machines. It is an option of those commands only, and for `convert` only of a picture (a convert to a drawing type draws nothing and sets no text, so the
option is a mistake in the line there; `commands` sets no text, and `oracle` measures in the machine's faces for itself and leaves out the cells a stand-in face measured). A library or a test that sets no faces measures in the bundled
ones.

## `run`

Runs commands in order, in a new headless session, and prints one JSON line for each:

```
$ diacraft-cli run --cmd file.new --cmd page.add --params '{"name":"Second"}' --cmd document.inspect
{"command":"file.new","result":{"history":{"redo":0,"undo":0},"pages":[{"id":1,"name":"Page-1"}],"title":"Untitled","version":1},"step":0}
{"command":"page.add","result":{"id":2,"name":"Second"},"step":1}
{"command":"document.inspect","result":{"history":{"redo":0,"undo":1},"pages":[{"id":1,"name":"Page-1"},{"id":2,"name":"Second"}],"title":"Untitled","version":1},"step":2}
```

`step` counts from 0 over everything the run was given, in the order it was given. The result is the command's own. The
session starts with no document: begin with `--cmd file.new` (a hidden `file.new` would make the first line of output
something nobody asked for). The commands that read a file are `file.open {path}` (a drawing, stencil or template: the six VSDX types, the three `.diacraft` kinds, an `.odg`, an `.otg` or a flat `.fodg`, below), `file.new {template: path}`, `stencil.open {path}` and `insert.picture {path}`; `file.save` and `file.save_as {path}` write a drawing: a drawing is written whole beside its file and renamed over it, so a failed save leaves the old file, and a link at the path is refused unless it leads to the drawing's own file (gaps A5). A file that cannot be opened is an error that names the kind of trouble and never the path, and the document that was open stays open.

**Reading a ShapeSheet**. `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate` read a sheet and write nothing (a query is no step of the history and is not journaled): `diacraft-cli run --cmd file.new --cmd shape.drop --params '{"master":"process","page":1,"x":5,"y":4}' --cmd sheet.cell --params '{"id":1,"cell":"Width"}' --cmd sheet.evaluate --params '{"id":1,"formula":"1+"}'` drops a Process shape and prints, for its width and for a formula that does not finish, lines such as

```
{"command":"sheet.cell","result":{"chain":[{"holds":false,"source":"local"},{"holds":true,"id":1,"name":"Process","shape":5,"source":"master"}],"class":"shape-only","default":{"formula":null,"shown":"0.0000 in","value":0.0},"error":null,"formula":"1.5 in","from":{"id":1,"kind":"master","name":"Process","shape":5},"guarded":false,"kind":"length","listed":true,"name":"Width","origin":"inherited","parsed":true,"ref":"Width","sheet":{"id":1,"kind":"shape","page":1},"shown":"1.5000 in","truncated":false,"unit":"IN","value":1.5,"worked_out":true},"step":2}
{"command":"sheet.evaluate","result":{"budget_exhausted":false,"error":null,"parse_error":{"kind":"unexpected_end","offset":2,"words":"the formula ends too soon: something is due at character 3"},"sheet":{"id":1,"kind":"shape","page":1},"shown":"","steps":1,"unit":null,"value":null,"worked_out":false},"step":3}
```

The sheet is named by `id` (with `page`), `page`, `master` (with `id`), `style` or `document: true`, and a cell by the reference a formula uses (`Width`, `Geometry1.X2`, `Prop.Cost.Label`); `sheet.read` pages through a sheet with `from` and `count` (at most 2000 lines). See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".

**Setting ShapeSheet cells**. `sheet.set_cell` and `sheet.set_cells` write a cell of a sheet as one undo step (the journal records them; a text that is the cell's own is no step): `diacraft-cli run --cmd file.new --cmd shape.drop --params '{"master":"process","page":1,"x":5,"y":4}' --cmd sheet.set_cell --params '{"id":1,"cell":"Width","formula":"3"}'` drops a Process shape and writes `3` in its Width, which reads `3 in` (a bare number is read in the unit the cell shows); the answer holds `{sheet, cell, changed, formula, value, unit, shown, error, origin, worked_out, recalculated}`. The same call again answers `changed: false` and `recalculated: {cells: 0, ...}`, and the first one worked out nine cells that read the width (`LocPinX`, the text block's cells, the geometry's `X` cells and the connection points'). An empty `formula` clears the cell's own formula and `force: true` writes past a `GUARD`; a refusal stops the run with a line that names the step, `diacraft-cli: step 4 (sheet.set_cell): invalid parameters for sheet.set_cell: the formula has an error: ...`, and exits with status 1. See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".

**Rows and sections of a ShapeSheet**. `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section` add and delete them, one undo step each: `diacraft-cli run --cmd file.new --cmd shape.drop --params '{"master":"process","page":1,"x":5,"y":4}' --cmd sheet.add_section --params '{"id":1,"section":"Scratch"}' --cmd sheet.add_row --params '{"id":1,"section":"Scratch"}'` gives the Process shape a Scratch section of one row and then a second row, whose answer is `{sheet, section: "Scratch", label: "2", row: 2, type: null, cells, renumbered}` (`cells` are the six cells a Scratch row holds, each at its default). With `--cmd sheet.set_cell --params '{"id":1,"cell":"Scratch.B1","formula":"Scratch.A2*2"}'` before a `sheet.add_row` with `"before":2`, the new row is row 2, the old row 2 is row 3, and the formula of `Scratch.B1` is written again as `Scratch.A3*2` and still reads 14: `renumbered` says `{rows: 1, formulas: 1, connects: 0, copied: {rows: 0, cells: 0}, work: {visited: 1, parsed: 1, located: 1}}`. A Process shape's own Geometry is its master's: `sheet.add_row` with `"section":"Geometry1","after":2` first takes a copy of its five rows as the shape's own, then puts the new row third, and says so in `renumbered`: `{rows: 3, copied: {rows: 5, cells: 15}, ...}` (three rows moved up one; a later edit of the master's rows no longer reaches this shape). A Geometry section made with `"variant":"Ellipse"` holds that one row alone, so `sheet.add_row` on it stops the run with `diacraft-cli: step 3 (sheet.add_row): invalid parameters for `sheet.add_row`: Geometry2 holds an Ellipse row, and a Geometry section that holds an Ellipse or an Infinite Line row holds that row alone: add the row to another Geometry section (sheet.add_section)` and exits with status 1. See [Editing commands](editing-commands.md), "The ShapeSheet (SHEETWIN)".

**Connection points**. `shape.connection_edges` reads the edges of a shape's outline and the connection points on each, and what a doubling of each edge would make under a minimum distance between points, and writes nothing (a query is no step of the history and is not journaled): `diacraft-cli run --cmd file.new --cmd shape.drop --params '{"master":"rectangle","page":1,"x":3,"y":2}' --cmd shape.connection_edges --params '{"id":1,"min_gap":0.3}'` tells the Rectangle's four sides, one point on each (`points`), the smallest gap a doubling would make (`gap`, paper inches) and the points it would make (`adds`: 2 on the long sides, and 0 on the short ones, where the gap of 0.25 inch is under the 0.3 asked for). The last line:

```
{"command":"shape.connection_edges","result":{"min_gap":0.3,"page":1,"scale_note":null,"shapes":[{"can_double":true,"edges":[{"adds":2,"edge":0,"from":{"x":2.25,"y":1.5},"gap":0.375,"kind":"straight","length":1.5,"path":[{"x":2.25,"y":1.5},{"x":3.75,"y":1.5}],"points":[2],"rows":[2,2],"section":"Geometry1","steps":1,"to":{"x":3.75,"y":1.5}},{"adds":0,"edge":1,"from":{"x":3.75,"y":1.5},"gap":0.25,"kind":"straight","length":1.0,"path":[{"x":3.75,"y":1.5},{"x":3.75,"y":2.5}],"points":[3],"rows":[3,3],"section":"Geometry1","steps":1,"to":{"x":3.75,"y":2.5}},{"adds":2,"edge":2,"from":{"x":3.75,"y":2.5},"gap":0.375,"kind":"straight","length":1.5,"path":[{"x":3.75,"y":2.5},{"x":2.25,"y":2.5}],"points":[0],"rows":[4,4],"section":"Geometry1","steps":1,"to":{"x":2.25,"y":2.5}},{"adds":0,"edge":3,"from":{"x":2.25,"y":2.5},"gap":0.25,"kind":"straight","length":1.0,"path":[{"x":2.25,"y":2.5},{"x":2.25,"y":1.5}],"points":[1],"rows":[5,5],"section":"Geometry1","steps":1,"to":{"x":2.25,"y":1.5}}],"id":1,"other":[],"points":4,"refused":null,"truncated":false}],"units_per_inch":1.0},"step":2}
```

See [Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)".

**Doubling and halving connection points**. `shape.connection_points` puts a new connection point in the middle of every portion of the edges it is given and moves nothing (`"action":"double"`), or takes away the points of the newest doubling of each edge and never an original one (`"action":"halve"`), as one undo step, with every connector kept glued (`edge` names one edge by the number `shape.connection_edges` gives it, `at` the edge nearest a place on the page, and neither every edge of every shape named): `diacraft-cli run --cmd file.new --cmd shape.drop --params '{"master":"rectangle","page":1,"x":3,"y":2}' --cmd shape.connection_points --params '{"id":1,"action":"double","edge":2}' --cmd shape.connection_points --params '{"id":1,"action":"halve"}' --cmd shape.connection_points --params '{"id":1,"action":"halve"}'` doubles the Rectangle's top (1 point becomes 3, 4 in all become 6), halves it back (generation 1, the two points the doubling made) and then is refused, in words and with exit status 1, because nothing is left to halve. The lines of the last three commands:

```
{"command":"shape.connection_points","result":{"action":"double","changed":true,"min_gap":0.11811023622047245,"page":1,"reglued":[],"renumbered":0,"scale_note":null,"shapes":[{"edges":[{"after":3,"before":1,"edge":2,"generation":1,"kind":"straight"}],"id":1,"points_after":6,"points_before":4,"skipped":null,"skipped_edges":[]}]},"step":2}
{"command":"shape.connection_points","result":{"action":"halve","changed":true,"min_gap":0.11811023622047245,"page":1,"reglued":[],"renumbered":0,"scale_note":null,"shapes":[{"edges":[{"after":1,"before":3,"edge":2,"generation":1,"kind":"straight"}],"id":1,"points_after":4,"points_before":6,"skipped":null,"skipped_edges":[]}]},"step":3}
diacraft-cli: step 4 (shape.connection_points): `shape.connection_points` failed: nothing to halve: the edge has no points a doubling made
```


**Offset and Fit Curve**. `shape.offset {ids, distance}` makes the two sides of each shown section of the shapes named (a copy outside and one inside a closed path, one left and one right of an open one, with the formatting of the shape it copies, no fill and no text) and keeps the shapes, and `shape.fit_curve {ids, tolerance, flags?}` makes the paths of the shapes named of fewer segments within the tolerance (`flags`: 1 periodic splines, 2 circular arcs, 4 break at abrupt changes, a sum) and keeps them the same shapes. Both take their number in the cells' own units (inches on a page of the default scale), answer `{page, ids, deleted, kept, changed, not_fitted}` like the other shape operations (`ids` the shapes made, `changed` the shapes a fit made smaller, `not_fitted` the shapes it left as they were that hold something to fit), and each is one undo step; a Fit Curve that changes nothing is none. `diacraft-cli run --commands offset.jsonl` with the steps below draws a rectangle and offsets it by a quarter of an inch (the copies are shapes 2 and 3), and `diacraft-cli export` of the file the steps save draws them:

```
$ cat offset.jsonl
{"command":"file.new"}
{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":4.0,"y":4.0}}
{"command":"shape.offset","params":{"ids":[1],"distance":0.25}}
{"command":"file.save_as","params":{"path":"offset.diacraft"}}
$ diacraft-cli run --commands offset.jsonl | tail -2 | head -1
{"command":"shape.offset","result":{"changed":[],"deleted":[],"ids":[2,3],"kept":null,"not_fitted":[],"page":1},"step":2}
$ diacraft-cli export offset.diacraft --out offset.png
```

A distance of 0 or less, or one that is not a number, or under 0.0001 or over 350,000,000, is refused in words, as is a path that crosses itself in too many places (the library's limits, shared among the command's calls). See [Editing commands](editing-commands.md), "Offset and Fit Curve".

**The six types**. `file.open` reads `.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx` and `.vstm`, the type being the package's own (the content type of its document part) and not the extension; an extension that disagrees is a warning. **A template opens as a new drawing made from it**: the document is the template's content and the result says `file.new_from_template: true`, but the drawing has no file (`file.save` is an error that says to use `file.save_as`), the first edit makes it unsaved, and the template on the disk is never written; the drawing keeps no file's bytes, so the template's other parts are the model's and the first file saved from it, a `.diacraft` file or a VSDX type written from the model, has times of its own. `file.open {path, edit_template: true}` opens the template file itself, to change it, and `file.save` then writes it. `file.save_as` takes the type from the extension; a Save As between the six rewrites the document's content type and keeps every other part (a drawing saved as a template, a template as a drawing, a drawing as a stencil, which keeps its pages and says so), and a stencil saved as a drawing or a template gets a part for each page that had none. **Macros** (`.vsdm`, `.vssm`, `.vstm`): the VBA project and its signatures are kept as bytes by every save, never run and never read. A file that holds a project, saved as `.vsdx`, `.vssx` or `.vstx`, is refused with `EngineError::MacrosNotAllowed` and nothing is written; `file.save_as {path, drop_macros: true}` leaves the project, its signatures and their relationships out of the new file and says so in `warnings` (`macros_dropped` counts the parts). That the reference app does the same is not known: it rests on the packaging and preservation specs and is on the check list of choices to compare.

**A `.diacraft` file** (DiaCraft's own format). `file.open` also reads one, told by its bytes first and not its extension (`file`'s `variant` is `diacraft`, `diacraft-stencil` or `diacraft-template`), and `file.save` and `file.save_as` write one: `diacraft-cli run --cmd file.open --params '{"path":"a.vsdx"}' --cmd file.save_as --params '{"path":"a.diacraft"}'` opens a VSDX drawing and saves it as `.diacraft`, which is then the drawing's own file. The three kinds are chosen by the extension of the name (`.diacraft`, `.diacraft-stencil`, `.diacraft-template`); a `.diacraft` drawing is saved as a VSDX type from the drawing alone (the thumbnail, `Application` and `modified` are the file's while the drawing is the one its VSDX file held, and a drawing made from a template, which no file of its own was read for, is written with a new file's); it is saved as an `.odg` or an `.otg` from the drawing alone too (the `.odg` model writer; the thumbnail and the generator are the file's while the drawing is the one the file held, and a drawing made new is written with a new file's), and a drawing opened from a VSDX file is saved as an `.odg` or an `.otg`, and one opened from an `.odg` as a VSDX type, by the model writer of the target (a save across the formats: below). **A drawing opened from an `.odg` is saved as `.diacraft`** (`diacraft-cli run --cmd file.open --params '{"path":"a.odg"}' --cmd file.save_as --params '{"path":"a.diacraft"}'`; the file holds everything the `.odg` held, and keeps no original), and from the `.diacraft` drawing it goes on to a VSDX type. **A new drawing is a `.diacraft` drawing**: `file.new`, blank or from a template, keeps its model and no file's bytes, its first `file.save_as` to a `.diacraft` name writes the package from the model and copies nothing, and to a VSDX type writes the file from the model alone (`diacraft-cli run --cmd file.new --cmd file.save_as --params '{"path":"a.vsdx"}'`); a drawing opened from a `.vsdx` is saved by `file.save` as a `.vsdx`, silently, and `file.save_as` to a `.diacraft` name is the way to the new format. `diacraft-cli export` reads a `.diacraft` file too. See [Editing commands](editing-commands.md), "`.diacraft` files".

**An OpenDocument drawing** (`.odg`). `file.open` also reads one, told by the package's own type and not the extension (`file.open`'s `file` says `variant: "odg"`),
and `file.save` and `file.save_as` write it by splicing the edit into the file's own bytes, so what an edit did not touch is the file's: `diacraft-cli run --cmd file.open --params
'{"path":"a.odg"}' --cmd shape.move --params '{"ids":[1],"dx":1,"dy":0}' --cmd file.save` moves a shape of a LibreOffice drawing and writes it back. A drawing is saved in its own
format: an `.odg` is saved as an `.odg`, an `.otg` or as `.diacraft`, a `.diacraft` drawing or a new one as an `.odg` or an `.otg` from the drawing alone, and a VSDX file as an `.odg` or an `.otg` too, and an `.odg` as a VSDX type (a save across the formats: what the file written cannot say rides inside it, and the save result says what, in `carried`). **A change the `.odg` writer does not write is a refused save in its
own words**, with nothing written and the file as it was: the step is named and the words follow, for example `diacraft-cli: step 2 (file.save): ... the drawing could not be saved: saving does
not yet write this change: a dashed line (...)`. The thumbnail of an edited save is rendered, and `warnings` say what a new shape has that the file does not carry (only the text formatting a style sheet gives the text and the link to the Theme style, each by its kind, with how many shapes and the first: rounded corners, a shadow, a text block that is not the whole shape, shape data, the layer, other cells and sections and the text itself are carried), what a copy of a shape held that a new shape cannot, and that a signed drawing's signature no longer matches; **every refusal but one for memory (which has its own words and its own way out), one for a shape an undo put back (which says to redo, `edit.redo`) and one for a group with no name that holds data (which names the group and says to name it, to put its data on a shape beside it or to use Save As for a `.diacraft` file) ends with the way out, `Undo the change (edit.undo) to save`**, and the answers of `connector.add` and `connector.style` carry no `warnings` list for a curved connector (the save writes it). A drawing template (`.otg`) opens as a new drawing made from it (below). **The open says what the drawing does not hold**: the first of `file.warnings` is then the line `What DiaCraft does not hold of this file: ...`, and a line for each class with its count follows (shapes of kinds kept only as placeholders, shapes in hyperlinks, font declarations that repeat a name, style properties no copy writes, and the like); a save into the file keeps all of it, and a copy saved as another file is without it.
**An `.odg` from the model, and the `.otg`**. `diacraft-cli run --cmd file.open --params '{"path":"a.diacraft"}' --cmd file.save_as --params '{"path":"a.odg"}'` writes an `.odg` from a `.diacraft` drawing, and so does a `file.new` drawing, blank or from a built-in template, saved to an `.odg` name (the package data of DiaCraft's own VSDX skeleton, which a template drawing holds, is left out: it is no person's): the `.odg` model writer makes the package from the drawing alone (no file to splice into), and the file written is the drawing's own, so a later `file.save` splices into it. The save's answer is the `.odg` save's (`variant` `odg` or `otg`, `kind`, `form: "package"`, `edited`: whether the drawing is not the content a file it was read from held). **The thumbnail and the generator follow the content stamp** and never the dirty flag: an `.odg` that visited `.diacraft` and comes back unedited keeps the thumbnail and the generator it was read with, an edit gives the thumbnail of an edited save and DiaCraft's generator, a drawing made new has no stamp and is given a rendered thumbnail. What the `.odg` cannot say of a drawing rides inside the file (a VSDX file's package data and the window settings among it), and the save's `carried` says what; a background page (a master page that holds shapes) added to the drawing is refused in words that say what is missing and name `.diacraft` and `.vsdx` as the ways out, with nothing written. **An `.otg`** (a drawing template) is a format to open and save and no built-in template: `file.open` of one makes a new unnamed drawing from it (`file` says `variant: "otg"`, `kind: "template"`, `new_from_template: true`), `edit_template: true` opens the template itself, and `file.save_as` to an `.otg` name writes a template (to an `.odg` name, a drawing). **A flat OpenDocument drawing** (`.fodg`, one XML document) is opened by `file.open` by what the file is and not by its name, as the model of the package that holds the same elements: `file` says `form: "flat"` (`"package"` for an `.odg`) and gives first, among `warnings`, a note of what Save does with it. **`file.save` writes it back as a flat file, in place**, whatever the file is called: unedited it is the file's own bytes (`edited: false`), an edit is spliced into the one document (what the edit did not touch is the file's own markup), and the answer says `form: "flat"`. A change the flat writer does not write (a master a stencil shape brings, a page added) is a refused save in words with nothing written, as for an `.odg`. **`file.save_as` writes it as another type**: a `.diacraft` file, an `.odg` or `.otg` package or a VSDX type, from the model, and the flat file stays as it was; `file.save_as` of it, or of any other drawing, to a `.fodg` name says Save As does not offer it. `export` draws a flat drawing as it draws a package.
The save's answer is `{bytes, variant: "odg", kind, form, edited, macros_dropped: 0, dirty: false, warnings, warning_count, notes, carried, checked}` (`notes` is empty, or holds the save note: what the drawing holds that a LibreOffice save would drop; it is not a warning; `carried` and `checked` are the fields of the table under `convert`); `edited` is false for a drawing saved again with nothing changed, which
writes the same bytes. See [Editing commands](editing-commands.md), "`.odg` files".

**Asking before a save**. `diacraft-cli run --cmd file.open --params '{"path":"a.vsdx"}' --cmd file.check_compatibility --params '{"path":"a.odg"}'` prints what `file.save_as` of that path would carry and lose, `{writer, format, variant, carried, checked, bytes}`, and writes no file and changes nothing in the session: `writer` says which writer would make the file (`diacraft`, `vsdx_model`, `vsdx_splice`, `odg_model` or `odg_splice`), `carried` and `checked` are the fields the save's result holds (`checked` is `null` for a splice into the drawing's own format), and `bytes` is the size of the file. It takes `file.save_as`'s `drop_macros`, `drop_objects` and `force`, and with no `path` it asks about the drawing's own file, as `file.save` would save it. A save that would be refused ends the step in the save's words, with `file.check_compatibility` named as the step. It costs what the save costs less the disk. See [Editing commands](editing-commands.md), "".

A step that fails ends the run. The failing step is named on standard error, and the status is 1; the lines of the steps
before it were already printed, and the steps after it do not run:

```
$ diacraft-cli run --cmd file.new --cmd page.rename --params '{"id":99,"name":"x"}' --cmd page.add
{"command":"file.new","result":{"history":{"redo":0,"undo":0},"pages":[{"id":1,"name":"Page-1"}],"title":"Untitled","version":1},"step":0}
diacraft-cli: step 1 (page.rename): invalid parameters for `page.rename`: no page has id 99
```

A failed command changes nothing (see the engine's rule in [`AGENTS.md`](../AGENTS.md)), and the session is as it was.

**The page's routing settings and a connector's route**. `page.routing` says what a page holds of its routing (the style and direction, Separate, Overlap, Appearance, the line jump values and the four
spacing groups in paper inches, and for each setting whether the page holds it), `page.set_routing` writes it in one undo step, and `connector.path` gives a connector's corners in paper inches, what it draws, the routing
style it follows and its reroute setting ([Editing commands](editing-commands.md), "The page's routing settings"). A new `style` routes again the connectors that follow the page, and `routed` says how many. Set the page to the
flowchart style from left to right, with a quarter inch by three tenths between connectors and shapes, and read the route of the connector between two boxes:

```
$ cat steps.jsonl
{"command":"file.new"}
{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}
{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":9.0,"y":6.0}}
{"command":"connector.add","params":{"from":{"id":1,"point":3},"to":{"id":2,"point":1}}}
{"command":"page.set_routing","params":{"style":"flowchart","direction":"left_to_right","connector_to_shape":{"x":0.25,"y":0.3}}}
{"command":"connector.path","params":{"id":3}}
$ diacraft-cli run --commands steps.jsonl | tail -1
{"command":"connector.path","result":{"editable":true,"hand_edited":false,"id":3,"page":1,"points":[[3.75,4.0],[6.0,4.0],[6.0,5.999999999],[8.25,5.999999999]],"reroute":"freely","route_style":{"code":6,"direction":"left_to_right","own":false,"style":"flowchart"},"style":"right_angle"},"step":5}
```

The connector draws a right-angle route whatever the direction (a directional style leans, and the router that follows will take it up). A direction the style does not take is refused in words, and the run stops there:
`diacraft-cli: step 1 (page.set_routing): invalid parameters for `page.set_routing`: the style `right_angle` takes no direction: leave `direction` out`.

**Reverse Ends**. `connector.reverse_ends` swaps the begin and the end of the connectors and lines it is given, in one undo step, each end staying glued to what it was glued to and the path walked the
other way, so the arrow ends up at the other end ([Editing commands](editing-commands.md), "Reverse Ends"). `id` names one shape and `ids` several (at most 4,096); a shape that has no begin and end is left alone and
listed in `skipped`, one whose `LockBegin` or `LockEnd` is on in `locked`, and a call that reverses none stops the run, in words. These are real lines: the connector between two boxes is reversed together with a box,
and `connector.path` reads the route back from the other end.

```
$ cat steps.jsonl
{"command":"file.new"}
{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}
{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":9.0,"y":6.0}}
{"command":"connector.add","params":{"from":{"id":1,"point":3},"to":{"id":2,"point":1}}}
{"command":"connector.reverse_ends","params":{"ids":[3,1]}}
{"command":"connector.path","params":{"id":3}}
$ diacraft-cli run --commands steps.jsonl | tail -2
{"command":"connector.reverse_ends","result":{"begin":{"id":2,"point":1},"end":{"id":1,"point":3},"id":3,"ids":[3],"page":1,"reversed":1,"says":"Reversed shape 3: its begin and end are swapped, with their glue, and the arrows stay on the ends of the path. Left as it was: shape 1.","skipped":[{"id":1,"why":"shape 1 is not a connector or a line: it has no begin and end to swap"}]},"step":4}
{"command":"connector.path","result":{"editable":true,"hand_edited":false,"id":3,"page":1,"points":[[8.25,6.0],[6.0,6.0],[6.0,4.000000001],[3.75,4.000000001]],"reroute":"freely","route_style":{"code":1,"direction":null,"own":false,"style":"right_angle"},"style":"right_angle"},"step":5}
```

The connector now begins at the second box and ends at the first, and its route runs from 8.25, 6 back to 3.75, 4: the line it was, the other way. The window's key for it, Ctrl+Alt+R, is listed with the command by `diacraft-cli commands`.

### Where steps come from

| Option | Step |
|---|---|
| `--cmd ID` | The command `ID`, with `{}` as its parameters. |
| `--params JSON` | The parameters of the `--cmd` just before it: a JSON object. Once per `--cmd`, and not before the first. |
| `--commands FILE` | The steps in the file. |
| `--commands -` | The steps on standard input. Once per run. |

Both forms may be mixed and repeated; the steps run in the order the options are given. A `--commands` source is either a
JSON array of step objects or one step object per line (JSON Lines; blank lines are skipped, and a byte order mark at the
start is ignored). A step object has `command` (or `id`, which means the same; not both) and, if the command takes any,
`params`, an object:

```json
[
  {"command": "file.new"},
  {"command": "page.add", "params": {"name": "Second"}},
  {"id": "document.inspect"}
]
```

An empty source is no steps, which is not an error. Every source is read, and every step in it checked, before the first step runs. A step that is not an object, has no
command, has a `params` that is not an object, or has a key other than `command`, `id` and `params` stops the run before
anything has happened, and the error names the source and the step (`--commands steps.json: step 2 (entry 3): …`). What a
command does with its parameters is the command's to check, and happens when its step runs.

### Limits

The steps are untrusted input. A run takes at most **100,000 steps** and **64 MiB** of `--commands` text, counting all the
sources together (not each). A run over either is refused before any step runs.

Parsing JSON takes far more memory than the text is long (about 1.1 to 1.5 GiB for 16 MiB of small objects, and about 2 GiB for
16 MiB of objects nested a hundred levels deep; measured on the release binary), and a JSON array is
parsed whole before its steps can be counted. So two more limits keep the memory of a run in bounds:

- The array form takes at most **16 MiB in all**: the arrays of all the `--commands` sources together. An array over that is
  refused before it is parsed, and the error says to put the steps one to a line instead.
- A line of JSON Lines takes at most **16 MiB**, as one line of the MCP server does.

Anything bigger is written as JSON Lines, a line to a step, which is parsed a line at a time and is limited by the 64 MiB and
the 100,000 steps. A step may itself be large, and the parameters of every step are kept until the run ends: four lines of
16 MiB each are within the limits, and hold about 5.6 GiB when they are small objects (one such line made the process peak at
1,425 MiB, two at 2,848 MiB, so four hold four times one) and about 8 GiB when the objects are nested a hundred levels deep (one line
peaked at 2,059 MiB; four are four times that).
That is accepted: `run` reads the person's own file, every source is read before the first step runs, so a run that cannot get
the memory ends before it has done anything, and a hostile file can end the process and nothing else.

## `commands`

Prints every command as a JSON array, in the order the menus list them: its id, label, menu place, shortcut, parameter doc
and whether it can run (`enabled`, and `disabled_reason` when it cannot). With no document open the commands that need one
are listed as disabled.

A command's `params` says what it takes. The snapping commands (`shape.move`, `shape.resize`, `shape.rotate`, `shape.drop`, `connector.add`, `connector.reconnect`, `snap.at`) take `snap`: `true`, or an object with `zoom` and, by command, `lock` (`shape.move`: `"x"` or `"y"`, the move keeps to that page axis), `nudge: {tick}` (`shape.move`: one press of an arrow key, `dx` and `dy` giving the direction, one of them 0, `tick` inches when nothing is ahead, else the ruler tick at the zoom), `view: [x0, y0, x1, y1]` (`shape.resize`: the window's visible part of the page in inches, so a width or height stops at that of a shape in it, every indexed shape when it is left out), `keep: "proportions"` (`shape.resize`: a corner keeps the shape's proportions, a side keeps the opposite side and the other axis grows about its middle) and `radius` (`shape.rotate`, with `angle`: how far the pointer is from the pin in inches, for how far another shape's angle pulls and for the rotation handle's step). A command takes only the words it has (`zoom` and `view` are every command's, `lock` and `nudge` `shape.move`'s, `keep` `shape.resize`'s, `radius` `shape.rotate`'s) and refuses the others in words. `snap.at` takes `edges` (an array of `left`, `bottom`, `right` and `top`), `keep`, `lock`, `nudge` (`{direction, tick}`), `view` and `rotate` (`{angle, radius}`) beside a place or a box, and tells what the matching command would do. Every `snap` result carries `marks`: lines with the shapes they run to, equal gaps with their lengths and chains, sizes and angles, each with its label, and `landed` (a resize that keeps proportions) and `angle` (a turn), and, for `shape.move`, `shape.place` and `snap.at`, `corner_boxes`: how many of the moving shapes lined up by the box round their corners where the box round their paths was meant (a turned shape lines up by the box round the paths it draws; one whose paths the call's budget could not pay for, or that draws none, keeps its corners). A hit on a line along no page axis (a side of a turned shape's own, a slanted guide) has a `normal` in degrees and no `axis`, and its mark has `axis` null; a resize of a turned or flipped shape tells `frame`, the box it comes to (`centre`, `width`, `height`, `angle`), and `snap.at` refuses `connect` beside a box, `edges`, `nudge`, `keep` and `rotate` in words. See `docs/editing-commands.md`, "Snap, grid and guides".

```
$ diacraft-cli commands | head -12
[
  {
    "id": "app.version",
    "label": "Version",
    "menu": [],
    "shortcut": null,
    "params": "none",
    "enabled": true
  },
```

`app.version` answers `{app, version, git, year}`: the version, the build line (`git`, made from `git describe --tags --always --dirty` and the version by `apps/diacraft/build_info.rs`: `0.2.0` for a build of the tag `v0.2.0`, `0.2.0-dev+aab1664d` for a build made before that tag exists, whose describe text names the last release (`v0.1.0-1555-gaab1664d`), `0.2.0+12.aab1664d` twelve commits after the tag, `.dirty` last for an edit that is not committed, or `unknown` when the build had no git and no `DIACRAFT_BUILD_GIT`) and the year of that commit (`DIACRAFT_BUILD_YEAR`, then `SOURCE_DATE_EPOCH`, then 2026). The binary's build script takes them at build time and the command line hands them to the engine at its start, so the MCP server of `mcp --headless` says the same; with `--connect` the answer is the running app's.

## `mcp`

Runs the [MCP server](mcp.md) on standard input and output, until the client closes its end. A release also holds it as an MCP bundle (`diacraft-mcp-<version>-<system>-<cpu>.mcpb`) that an app which installs MCP servers starts as `diacraft-cli mcp`: [Installing it from a bundle or a directory](mcp.md).

| Option | Backend |
|---|---|
| `--headless` | A session of this process, with no window. |
| `--connect ADDR` | The running app's control channel, such as `127.0.0.1:7997`. Fails with status 1 if nothing answers there, or if what answers is not DiaCraft. |
| neither | Looks for the app at `127.0.0.1:7997` once. If DiaCraft answers, the server uses it; if nothing answers, or another program does, it says why on standard error and runs headless. |

`--automation-read-root DIR` and `--automation-write-root DIR` (each may be given again for more folders) keep the files the agent's commands read and write to those folders, in a session of this process: [Keeping files to the folders you grant](mcp.md#keeping-files-to-the-folders-you-grant). `DIACRAFT_AUTOMATION_READ_ROOT` and `DIACRAFT_AUTOMATION_WRITE_ROOT` give the folders of a kind that has no flag. A folder that is not there is status 2, `--connect` with a root flag is status 2, and the default mode with a root flag stops with status 1 if the app answers (give `--headless`). With none of them nothing is limited. `run`, `export`, `convert`, `roundtrip`, `oracle` and `compare` have no roots: they read and write the paths typed.

`--bundled-fonts` sets text in the bundled faces alone, in a session of this process (see [Fonts](#fonts)); the app has the faces it read, so
`--connect` with `--bundled-fonts` is an error (status 2) like `--connect` with `--headless`.

`--connect` and `--headless` together are an error (status 2).

The default mode never drives another program. Other craft apps listen on port 7979 by default, and DiaCraft on 7997, so a
program on 7997 that is not DiaCraft is not impossible. DiaCraft asks `ui.inspect` first and goes on only if the answer
says `"app": "diacraft"`; it sends nothing else to a program that says anything else.

## `oracle`

A harness over a folder of drawings: for each file it reads the bytes as a VSDX package, works every
formula cell out with `diacraft-calc`, compares the result with the `V` and `E` the file saved, and prints a report. It opens the file in nothing else.
**A report holds no formula text, no text from the file and no name a person made up**: it holds the SHA-256, the category, names that the spec's own lists
hold (the cells, the functions, the 17 sections, the reserved user rows and the triggers), counts, error classes, and values only as the value they are
(a number, a colour, a date) with a text shown as its length (`<text:12>`). A name a user made up (a `User` or `Property` row, a function a file defines,
a cell the spec does not list) is content: it is counted and never named. The harness is `apps/diacraft-cli/src/oracle/`: it joins the reader
(`diacraft-vsdx`) to the one comparer (`diacraft-calc`), and the format crate carries no harness, no name lists and no JSON.

```
$ diacraft-cli oracle --json --source owner/repo drawing.vsdx
{"schema":4,"sha256":"...","bytes":63505,"source":"owner/repo","read":true,"category":{...},"read_report":{...},"names":{...},"saved":{...},"formulas":{...},"calc":{"available":true,"control":{"passed":true,...},"cells":{...},"compared":812,"matched":790,...},"score":{"score":0.97,"matched":790,"compared":812,"coverage":0.41}}
```

**Every run begins with a salted control.** A score of 100% proves nothing by itself: a calculator that hands back the saved values, a comparer that compares nothing
and a walk that skips a sheet each give it. So the harness first builds a small document whose saved values and errors are known to be wrong, writes it as a package and
scores it through the function a file goes through. It passes only if every salted cell lands in `differed` (a wrong number, text and colour; an `E` the formula does not give;
a failing formula with no `E`; the wrong `E`; an `E` beside `Themed` or an empty `V`; an inherited cell; a cell in a row of a section; one on each kind of sheet and at each depth),
every honest cell agrees, and nothing was left out. **When it fails, the run has no score**: each report has `score: null` and `score_withheld: "control_failed"`, the
`calc.control` member says which cells missed, and the command prints every report and then exits with status 1. The words form begins with a line `control passed: ...` or `control FAILED: ...`.

**Every scored file is salted too.** The control is one synthetic document, and a calculator that hands back the values a file saved (an echo) agrees with every
cell of a real file: the control cells catch an echo, which would score 100% on real files. So after the walk the harness takes
a fixed, seeded sample of the real cells that **agreed**, puts a wrong saved value of the cell's own kind in each (a number a size or 1 away, a Boolean flipped, a text no
number reads as, a colour half a range off in every channel, a date a day later, a currency, point or constructor moved, a saved error replaced by another), works the cell
out again with the same calculator and compares it with the same comparer. The sample is the first agreed cell of each kind of value in the file and one in a thousand of the
rest, from a fixed seed, so a file with an agreeing cell has at least one and a second run salts the same cells. The salt is worked out on the budget the walk left and puts the
counters back, so it changes no figure of the score and cannot end a file as `budget_exhausted`. **A salted cell that still agrees fails the file**, but for `ATAN2`'s
zero-length rule (a call whose two operands read under 1E-6 agrees with any saved value, counted in `by_allowance`): its score is withheld
(`score: null`, `score_withheld: "real_salt_failed"`), the words form says `real-file salt ... (FAILED)`, and the command ends with status 1 after printing every report.

| Part of a report | What it says |
|---|---|
| `sha256`, `bytes`, `source` | The file, as a run over many files names it, and where it came from (`--source`). |
| `refusal` | When the file would not open: the class word (`not_zip`, `zip_corrupt`, `path_traversal`, `no_drawing_parts`, `xml_malformed`, `too_many_shapes`, ...), the kind (`not_a_package`, `too_big`, `damaged`) and a `detail` in words from a fixed set and numbers: for `zip_corrupt` the rule of the ZIP reader that fired; for `invalid_part_name` the rule; for `no_drawing_parts` which case (no relationship, no part, a root that is not the document element, a document element in a namespace the reader does not read); for an XML class the fault (`not well-formed at byte N`, `more than N items`, `nested deeper than N`, ...) and the part; for a limit of the reader the cap and the part and section it was reading. **A part is named only when its name is package structure** (letters, digits and `/ . _ -`, 80 bytes at the most); any other is `an XML part` or `a relationships part`, as a name a person typed is content, and a section the file made up is `<other section>`. A refused file is a report: it has `read: false`, its hash, and no cells. |
| `category` | `drawing`, `stencil` or `template`; macros or not; the writer (`reference_app`, `other`, `unknown`, by `Application` of `docProps/app.xml` on the census's **exact** match of the reference app's own name, with or without its maker's name before it, no trimming and no folding of case); and the census's `oracle_usable` and `routing_usable` (with `glued_connectors`, how many connectors the routing term counted, a part that several pages share counted once), without its ZIP-level package traits, which the reader does not see (`package_traits: not_seen`). |
| `read_report.stats` | The sizes that confirm the caps of and of the reader's own: styles, masters, pages, shapes in each page, the deepest nesting of shapes, cells, formula cells and nodes, XML items and bytes, the most cells in one place, sections in a sheet and rows in a section, bytes of pictures; and, for the container's caps and the XML limits: the size of the file, the rows of the ZIP directory, the longest part name, the largest part, the rows of `[Content_Types].xml`, the largest `.rels` part and its most rows, the most items in one XML tree, the deepest nesting and the longest attribute. **A file that was refused has a `read_report` too**, with what the reader knew when it stopped. |
| `read_report.warnings`, `skipped` | Every warning and every item the model has no slot for, counted by class word. `skipped` is **exact and complete**: every place the reader drops something (a foreign attribute, a namespace declaration, a comment, an instruction or a child of an element it reads into a typed place) has a class, and a test injects each kind of thing at every position of a package and fails on one that is in neither the model nor this count. What is counted is in the file, which the session keeps (`file.open`, `Session::source`); it is not in the `Document`. |
| `names`, `saved`, `formulas` | The cells and functions in use and how, the saved `V` and `E` kinds, the units, what did not parse (by class) and how many formula nodes were used. |
| `calc.control` | What the salted control found on this run: `passed`, the salted cells caught and missed (by a fixed id such as `s03_page_sheet`), the honest cells that agreed. |
| `calc.cells` | **Every cell of the document is counted once.** `formula` (the cells with an `F` of their own or `Inh`), and `not_formulas` (a `V` with no `F`, `No Formula`, and every cell of a deleted section or row; `with_saved_error` counts those that carry an `E`, which nothing can check), and `in_document` is their sum. |
| `calc.compared`, `matched`, `differed` | The formula cells that were worked out and had a saved `V` or `E` to compare. The comparer is `diacraft-calc`'s alone: `E` first (so an `E` beside `Themed`, an empty `V` or none is still compared), then numbers within a relative 1E-9 (a floor of 1 on the size), a date within a second, text exactly, a colour by its channels. `agreed_by_allowance` counts the matched cells that an absolute 1E-9 would not have passed (a size above 1, the geometry batch's 1E-6, `MSOTINT`'s step, `ATAN2`'s two allowances, the curve below), so the score can be read both ways. `agreed_by_curve` counts, among them, the `C` and `D` cells of elliptical arcs that agree only because the pair they are part of describes the same ellipse as the file's (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`: the other end of an axis, the other axis with the reciprocal ratio, any angle for a circle); a raw match is not counted there. |
| `calc.left_out`, `left_out_total` | The formula cells left out, **by reason**: `action_cell`, `formula_not_parsed`, `reads_clock_or_machine` (`NOW`, `RAND`, the four `DOC*` dates, `FILENAME`, `DIRECTORY`, and any cell that reads one by a reference or a text read, by address), `themed_no_error` (the cell's own saved value is the `Themed` marker), `themed_operand` (the cell reads an operand saved as `Themed`, which is no value: 97,954 cells in the first recalculation score, counted before as `unsupported_unlisted`), `nothing_saved`, `inherited_no_formula`, `date_past_9999`, `date_past_9999_operand` (the cell reads a date past the year 9999), `stale_theme_colour` (a `THEMEVAL` colour under a scheme the file no longer selects, **narrowed in schema 7**: the cell is worked out with no theme and again with the file's first theme part selected, and is left out only when the saved colour is the second reading's and not the first's; every other cell under the rule's condition is scored, as a pass or a difference, so this count is the number the rule leaves out, which only a run can measure; a `THEMEPROP` on a sheet that reads no theme is scored now, and has no reason of its own), `machine_font_number` (a font cell, `Font`, `AsianFont` or `ComplexScriptFont`, whose evaluation gives a number other than 0: the number is a font number of the machine that saved the file), `theme_no_shadow` (a shadow property a theme answered on the branch where its effect entry holds no outer shadow: the entry decides, and not the value saved), `formula_must_fail` (the file saves a value and no `E`, and the cell's own formula reached a `REF()` or a cell its section can never hold and that `#REF!` is its result; a `#REF!` from a row, shape, page or master the file lacks is scored), `depth_limit` and `ambiguous_sheet` (a page, master or style whose `ID` an earlier one has). |
| `calc.stale_theme_colour_by_function`, `stale_theme_colour_by_function_unlisted` | The cells counted in `left_out.stale_theme_colour`, tallied by the functions their formulas call: a cell is counted once under each function it names (so a cell that calls `IF` and `THEMEVAL` is in both), by the spec's names, and the names the spec does not list together in `..._unlisted`. A jump in `stale_theme_colour` between two runs can be traced to a kind of formula here; a rule that leaves out a cell because the first theme part gives the colour the no-theme reading gives too can hide a bug in the no-theme colours, and this is where it shows. |
| `calc.not_calculated` | The formula cells that could not be: `unsupported` by function (named only when the spec lists it, else `unsupported_unlisted`: **a function the spec does not list, and nothing else**), and `unreached` when the budget ran out. |
| `calc.real_salt` | The real-file salt of this file: `salted` cells given a wrong saved value, `caught` (differed, as they must), `by_allowance` (agreed only through `ATAN2`'s zero-length rule), `missed` (agreed with no allowance to say why: a failure), `not_checked` (chosen, and the budget the walk left was spent), `too_large` (agreed, and a value too large to copy), `by_kind` (salted and caught for each of `number`, `date`, `bool`, `text`, `colour`, `currency`, `point`, `constructor` and `error`), and the first `misses` by sheet, the spec's cell and function names and the kind. |
| `calc.differences` | At most 50 differences, at most two of one kind of cell, each with the sheet by ids (`page`/`master`/`style` and its `id`, a shape's own `ID` and depth), the section and its `IX`, the row (`{"ix": 2}`, or `{"named": true}` and no name), the cell, `inherited`, the functions, the outcome (`value_differs`, `error_missing`, `error_unexpected`, `error_differs`, `value_and_error_differ`), and the expected and actual values. `by_function` and `by_cell` score every cell by the spec's names. |
| `score` | Cells matched over cells compared, with the `coverage` (compared over formula cells) beside it: a score over one cell is a score, and the coverage says it is one cell. `null` when nothing was compared, and when the control failed. |

Without `--json`, the same in a few lines of words. A file that is not a package is a report with its refusal and status 0. A file that cannot be read from disk
(there is no such file, a folder, no permission, larger than the package cap and its overhead) is named on standard error, and the status is 1 once the rest are done.
More than one file is one line each, in order.

`--aggregate` reads files of the lines `oracle --json` printed and prints one line of JSON, **totals first**: the reports read, the distinct files (one hash in two sources is one
file, kept under the lesser source name), the files refused, the files whose control failed (in no figure), the files whose real-file salt failed (`real_salt_failed`, in no figure either, and the command ends with status 1 after printing the line) and the files scored, with `real_salt` added up over the files that read; the cells compared, matched and left out by
reason, the cells not calculated, the cells with no formula, and the coverage. Then the score pooled over cells, by file, by source (each source counts once) and with each source
cut to its first `--cap` files by hash (default 5), of a count of files from repositories that share a writer; each source lists its files, its refused files and
its scored files. **A line that cannot be true stops the aggregate** with the file, the line and the reason: matched cells past compared ones, counts that do not add up to the
formula cells, a score that is not its counts, a score under a control or a real-file salt that failed, a real-file salt whose caught, allowed and missed cells are not the salted ones, or a report of an older schema (schema 3 counts a `Themed` operand as its own reason, so it is not added to schema 2; schema 4 is a score by the project's own rules, so it is not added to schema 3).

## `export`

Writes one page of a drawing as a picture, a vector file or a PDF, by the extension of `--out`:

```
$ diacraft-cli export drawing.vsdx --page 2 --out page2.svg
{"bytes":18211,"format":"svg","height":816,"out":"page2.svg","page":{"id":3,"name":"Floor plan"},"skipped":[{"count":1,"first":"shape 12: an EMF or WMF picture","kind":"MetafilePlaceholder"}],"width":1056}
```

| Option | What it does |
|---|---|
| `FILE` | The drawing, read as `file.open` reads it (a `.diacraft` file, told by its bytes first, a `.vsdx`, `.vsdm` and the like, or an OpenDocument drawing, `.odg`, told by the package's own type, or a flat `.fodg`, told by its content). A file that cannot be opened is an error that names the kind of trouble and never the path. |
| `--out PATH` | Where to write, and what: `.png`, `.svg` or `.pdf` (any case). The file is written whole or not at all (a temporary file beside it, then a rename), so a failure leaves a file that was there as it was. It is never the drawing itself (a link to the drawing is caught too). |
| `--page N` | The page's place in the drawing, from 1, as the file lists its pages (the default is the first page that is not a background: a file lists its background pages first, so `--page 1` of such a file is the background). The result line names the page's `ID` and name. |
| `--dpi N` | PNG only: pixels to the inch, 96 by default, at most 2400. The picture is the page's size at that resolution, and a page that would pass 16,384 pixels a side or 64 million in all is an error. |
| `--text MODE` | SVG and PDF only: `text` (the default) writes letters as text where the font allows it, and `outlines` writes every letter as paths. |
| `--transparent` | No white paper under the drawing. A PNG or an SVG has a clear page; a PDF is written without the white rectangle. |
| `--bundled-fonts` | Draw in the bundled faces alone, which are the same on every machine (see [Fonts](#fonts)). |

What each format holds:

* **PNG**: the page as the window draws it, in straight (not premultiplied) RGBA, with the physical size in the file.
* **SVG**: paths in CSS pixels (96 to the inch), gradients, clips, blurred shadows as a Gaussian blur filter, pictures as PNG data URIs, a shape that is drawn as a `<g>` whose `<title>` is its alt text title (its name when it has no alt text) and whose `<desc>` is its description, the page's alt text as the file's own `<title>` and `<desc>`, and each run of letters whose glyphs are one for one as a `<text>` element naming the face with an x for every letter; ligatures, joined and right-to-left scripts and a made-up bold or slant are outlines.
* **PDF**: one page of the page's size at 72 points to the inch, vector paths, gradients as shadings, clips, pictures, and letters as text in an embedded subset of the face (so the text can be selected and searched) where the face's licence allows embedding (`OS/2` `fsType`), else outlines. **A blurred shadow is drawn hard-edged** (PDF has no blur).

`skipped` lists what the page left out or drew by a stand-in, with how many and the first of them in words, as the window counts it (`Skipped` in `crates/render`): an EMF or WMF picture whose header cannot be read is a placeholder, one drawn without some of its records says which of eight details it is (`MetafileRecordSkipped` and the like), a gradient direction or a shadow type is an approximation, `TextAsOutlines` says letters that could not be written as text. The grid, rulers and guides are not in the picture, as in the window.

**Printing.** The engine's `file.print_preview {page?}` says what a print of the drawing would put on paper (for each page its paper, the way the sheet is held, the scale, the sheets across and down, and the runs of sheets that share a paper), and `file.print {path, page?, text?, color?}` writes what a printer would be given: a PDF with a page for each sheet at the paper's size, with the page's part for that sheet on it (`diacraft-cli run --cmd file.print` reaches it after a `file.open`). `color: grayscale` writes it in black and white, every colour as its grey (`0.299 R + 0.587 G + 0.114 B`, in `editing-commands.md`); `color` is the default. It has the rules of an export's path; it sends nothing to a printer, which only the window does. A print the drawing cannot have (margins that leave no room, a page that takes more than 1,024 sheets, a print that would hold more than 256 megabytes of drawing) is an error in words.

**Where it writes.** Exactly where `--out` says. The command line has no granted folders: the automation roots of gaps A2 (`--automation-read-root`, `--automation-write-root`) are for `mcp` and the app's control channel, and this command writes the path its person typed. The engine's `file.export` (the same export, for a client other than the person who typed the path: `{path, page?, dpi?, text?, transparent?}`; `diacraft-cli run --cmd file.export` reaches it after a `file.open`) has the rules of a save, and under roots a write root as well: a link at the path is refused unless it leads to the open drawing, and then the drawing itself is refused, the extension is judged on the file that would be written, and the open drawing is never written.

A mistake in the line (no `--out`, an extension that is not one of the three, an option for another format, a page below 1) is status 2 and writes nothing. A drawing that cannot be read or opened, a page the drawing does not have, a page too large for the format and a file that cannot be written are status 1, named on standard error.

## `convert`

Writes a drawing as another type, by the extension of `--out`. A drawing type is the engine's own Save As, so what is refused is refused in the engine's words; a picture type is `export`'s:

```
$ diacraft-cli convert drawing.vsdx --out drawing.diacraft
{"bytes":4446,"carried":{"classes":[],"document_level":false,"macros":false,"objects":false,"signatures_stale":0},"checked":{"as_values":false,"classes":[],"done":true,"reason":null},"copied":0,"format":"diacraft","in":"vsdx","macros_dropped":0,"out":"drawing.diacraft","parts":null,"variant":"diacraft","warning_count":0,"warnings":[]}
$ diacraft-cli convert drawing.diacraft --out drawing.vsdx
{"bytes":6332,"carried":{"classes":[],"document_level":false,"macros":false,"objects":false,"signatures_stale":0},"checked":{"as_values":false,"classes":[],"done":true,"reason":null},"copied":null,"format":"vsdx","in":"diacraft","macros_dropped":0,"out":"drawing.vsdx","parts":{"added":9,"kept":0,"removed":0,"rewritten":0},"variant":"vsdx","warning_count":0,"warnings":[]}
$ diacraft-cli convert drawing.diacraft --out page1.png --page 1
{"bytes":2028,"format":"png","height":425,"in":"diacraft","out":"page1.png","page":{"id":1,"name":"Page-1"},"skipped":[],"width":550}
```

| Option | What it does |
|---|---|
| `FILE` | The drawing, stencil or template, opened as `file.open` opens it: a `.diacraft` file, a VSDX package, an OpenDocument drawing or a flat `.fodg`, told by its bytes and not its name. **A template is converted as the file itself** (as `roundtrip` opens one, `edit_template`), not as a new drawing made from it. A file that cannot be opened is an error that names the kind of trouble and never the path. |
| `--out PATH` | Where to write, and what: the extension, in any case, names the type. A drawing type: `.diacraft`, `.diacraft-stencil`, `.diacraft-template`, `.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx`, `.vstm`, `.odg` or `.otg`. A picture type: `.png`, `.svg` or `.pdf`. Any other extension (`.fodg` among them, which no Save As writes) is a mistake in the line. The file is written whole or not at all, and **it is never `FILE` itself** (a link to it, and another spelling of its path, are caught too). |
| `--page N`, `--dpi N`, `--text MODE`, `--transparent`, `--bundled-fonts` | A picture's options, as `export` has them (the page's place from 1, and the first page that is not a background when it is left out, pixels to the inch of a PNG, letters as `text` or `outlines` in an SVG or a PDF, no white paper, the bundled faces alone). **For a drawing type each is a mistake in the line**: nothing is ignored. |
| `--drop-macros`, `--drop-objects` | A drawing type (a mistake for a picture): leave the macro project, or the embedded objects of the other format, out of the file written. They are options and never required: without `--drop-macros` a macro project that the type cannot hold (a VSDX type that allows no macros, whatever language the project is in) refuses the save in words that name `.vsdm`, `.vssm` and `.vstm` as the other way out; with `--drop-objects` each shape that showed an object shows its picture. The drawing keeps both: the `.diacraft` file saved from it holds them. |

What each type holds:

* **A drawing type** is `file.open` and then `file.save_as`, run through the registry as `run` runs them, so a convert is exactly a Save As: the file is made whole in memory, goes to a temporary file beside the target and is renamed over it (a failed save leaves a file that was there as it was), a link at the target is refused unless it leads to the drawing's own file, a macro project is refused where the type cannot hold one, and every warning is the engine's. The command makes no edit. **Which pairs the engine writes is the engine's to say** (`Session::save_as_allows` is its table asked without saving, and the tests hold the list of types here to it); today: a VSDX package to any of the six VSDX types, to `.diacraft` (`.diacraft-stencil`, `.diacraft-template`) and, across the formats, to `.odg` and `.otg`; a `.diacraft` file to the three `.diacraft` types, to the six VSDX types and to `.odg` and `.otg`, from its model (a new drawing too); an `.odg` or `.otg` to an `.odg` or `.otg` (its own bytes spliced, in the kind the name says), to `.diacraft` and, across, to a VSDX type; **a flat `.fodg` to every drawing type** (a flat file is no package to splice into, so it is written from the model as a `.diacraft` drawing is, and the `.odg` is a package and never a flat file; the `.fodg` is left as it was). Any stencil to an `.odg` or an `.otg` is refused in the engine's words (status 1, nothing written), and **no pair writes a `.fodg`**: it is no type of the list, and naming one is the mistake of `--out`. A VSDX package that went to `.diacraft` and back has the same content (`compare --content`), an `.odg` that went to `.diacraft` reopens as an equal model, and so does a flat `.fodg`, whose `office:meta`, `office:settings` and inline pictures come with it. The `.odg` made of a flat file is the same content as the flat file (`compare --content`, a flat file against a package) for a drawing whose parts the model writer writes as they were read, and the save's own `warnings` say what it does not write yet (the named definitions and the style families of other kinds).
* **A picture type** is `export`'s path: the page is drawn as the window draws it, and the bytes are the ones `export` writes for the same options.

The line is one object on standard output, with `in`, `out`, `format` and `bytes` for either kind:

| Field | What it says |
|---|---|
| `in` | The type of `FILE` as `file.open` names it (`vsdx`, `vsdm`, `vssx`, `vssm`, `vstx`, `vstm`, `diacraft`, `diacraft-stencil`, `diacraft-template`, `odg`, `otg`; `null` for a package whose type is none of them). |
| `out`, `format` | `--out` as it was given, and the type written: its extension in lower case. |
| `bytes` | The size of the file written. |
| `variant`, `macros_dropped` | A drawing type: the type the save wrote (the extension, without the dot), and how many parts of a macro project it left out. |
| `parts` | A drawing type: for a save of a VSDX type, `{kept, rewritten, added, removed}`, the counts of the parts of the package; `null` for any other. |
| `copied` | A drawing type: for a save as `.diacraft`, how many entries of the package were copied as they were from the file it was read from or last saved as; `null` for any other. |
| `carried` | A drawing type: what the file carries of the data of the other format, the engine's own field `{classes, document_level, macros, objects, signatures_stale}` (`docs/diacraft-format.md`, 7.7): the classes in plain words ("line ends", "gradients", "style names", "window settings", ...), whether some of it is in DiaCraft's document data, whether a macro project or objects ride, and how many digital signatures no longer match the file. Empty classes and `false` for a save that carries nothing. It is never a warning. |
| `checked` | A drawing type: what the check after the save found, the engine's own field `{done, classes: [{class, words, count}], as_values, reason}` (`docs/diacraft-format.md`, 7.7): after a save through a model writer (a VSDX file or an `.odg` written from the model, a `.diacraft` file written from a drawing of another format or made from a template file of one) the file is read back and compared with the drawing, class by class, and `classes` says each class that differs with the number of its items; `as_values` says that content is written in the forms the format holds (units, `Width*1` for `Width`, values from style sheets, links to style sheets), which is no difference; a formula that does not come back as a formula is counted in `formulas`; `done` is false, with `reason`, for a file written that could not be checked. `null` for a save into the drawing's own file, which is not checked in 0.2. It is never a warning. |
| `warning_count`, `warnings` | A drawing type: how many warnings the save gave, and the first 20 of them, each as `{class, words}`. `class` is a fixed word a program reads (`field_as_text`, `style_links_not_written`, `text_format_not_written`, `macro_project_dropped`, `signature_no_longer_holds`, ...: one for every kind of warning a save gives, the VSDX writer's, the `.odg` writer's notes (one for each variant of `SaveNote`), what the engine adds and the `.diacraft` writer's, which `diacraft-cli` holds to the sources of the warnings by a test), and **`other` only for a warning that no class knows**, which a person reads in the window's save note. `words` says what changed and where: **the engine's own sentence** for a warning that holds only words and numbers (the `.odg` writer's notes and the engine's own, which count shapes and name them by their numbers: `2 fields of shape 3 on page 0 have no element in an .odg that is read back as a field, ...`), and **a sentence of the class** for a warning that may name a part or hold the words of an error (the VSDX writer's), which holds nothing of the warning. `roundtrip` tells only the class word, as it always did: its report may cross to a person who must not see the file. |
| `page`, `width`, `height`, `skipped` | A picture type: `export`'s fields (the page's `ID` and name, the size in the format's own units, and what the page left out or drew by a stand-in). |

**Where it writes.** Exactly where `--out` says, as `export` does: the command line has no granted folders (the roots are `mcp`'s and the control channel's). A convert never writes its input, and it makes one file.

A mistake in the line (no `FILE`, no `--out`, an extension that is not one of the types above, an option for another type, a page below 1) is status 2 and writes nothing. A file that cannot be read or opened, an engine refusal, a page the drawing does not have and a file that cannot be written are status 1, named on standard error in the engine's words or `export`'s, and never with the path. Batch conversion (many files, an output folder) is not part of `convert`.

## `roundtrip`

```
$ diacraft-cli roundtrip drawing.vsdx --out copy.vsdx --move 0.5,-0.25
{"changed":{"other":[],"shape_count":1,"shapes":[{"cells":["PinX","PinY"],"other":[],"page":1,"sections":[],"shape":5,"under":[]}]},"model":{"differ":["page 1"],"differ_count":1,"equal":false,"reads_back":true},"moved":{...},"opened":true,"parts":{"changed":["/docProps/app.xml","/…/pages/page1.xml"],"changed_count":2,"identical":21,"in":23,"out":23,...},"saved":true,"sha256":"...","variant":"vsdx",...}
```

The tool for the round trip of many files of the VSDX formats. It opens `FILE` as `file.open` does (a template as the file itself, `edit_template: true`), optionally moves the first top-level shape of the first page that has one by `--move DX,DY` inches (`shape.move`), saves to `--out` as `file.save_as` does (the type is the extension of `--out`), and prints **one line of JSON** about what the save changed. `--out` is never `FILE` (a file of a run is not to be written); a link to it is caught too.

| Field | What it says |
|---|---|
| `sha256`, `bytes`, `variant` | The input: its SHA-256, its size, and its type (`vsdx`, `vsdm`, `vssx`, `vssm`, `vstx`, `vstm`; `null` when the content type is none of them). |
| `opened`, `refusal` | `false` with `refusal.class` (the reader's class word) when the file would not open; if the engine refused a file the reader had read, `class` is `engine`, with `error` (the kind of engine error: `cannot_open`, `failed`, `bad_params`...) and `problem` (the kind of trouble opening was: `damaged`, `too_big`, `out_of_memory`...: `too_big` is a file past a limit of the reader or one that the whole memory a read may use, 2.6 GB for the process, cannot hold (told by how far the refused read got, in its XML and not its pictures, and only when that is clearly past the budget: the doubt is for the file), and `out_of_memory` is a file that would be read alone but is not beside what the process holds, the model and the file the run keeps for the comparison included). The status is 0, so a run over many files goes on. |
| `moved` | With `--move`: the page and shape moved and how many `shape.move` moved, or `error`, the kind of engine error as a word. |
| `saved`, `save`, `save_error` | `file.save_as`'s result (bytes, type, how many parts were kept, rewritten, added and removed, `warning_count`, and `warnings`: the class word of each of the first 20 warnings, such as `property_part_kept`, `parts_without_content_type`, `stencil_keeps_pages`, `macro_project_dropped`, `signature_no_longer_holds`, or `other`), or `save_error.error`, the kind of engine error as a word (`macros_not_allowed` when a macro project would be lost, `failed`...). |
| `parts` | The two packages part by part: `in` and `out` counts, `identical` (the same uncompressed bytes), and the parts that `changed`, were `added` or were `removed` (at most 50 of each, with the count). A name that is package structure is shown (a page part's, say); any other part is called `a part`. `content_types` and `relationships` say whether the rows are the same (relationships by part, as sets of `Id`, `Type`, `Target` and mode). |
| `model` | Whether `--out` reads back as `FILE` read, but for the relationship ids a save writes of its own: `equal`, and what `differ` (`page 3`, `master 2`, `styles`, `document_sheet`...). |
| `changed` | The shapes whose cells differ between the two models: the page, the shape's `ID`, the `ID`s of the groups above it, the names of the cells under it that differ, the predefined sections in which a cell or a row differs (`Geometry.row2.X`), and `other` (`text`, `data`, `picture`, `identity`, `links`, `unknown`). With no `--move` it should be empty. |

What may cross to a person who must not see the file is the hash, the type, counts, part names that are package structure (a part whose name holds anything but letters, digits and `/ . _ -`, or is long, is called `a part` or by its role: a name can hold a person's words), page and shape `ID`s, cell names in the spec's own spelling (any other is `(unlisted)`), the 17 predefined section names, and the class of each warning of the writer and of each error. **No formula, no value, no text and no name a person made up** is in the line. **No word the engine or the writer said is copied**: the fields of the save's result are picked, an error is told by its kind and a warning by its class (a fixed word from a table in the command, found by a phrase that no part name can hold; `other` for a warning the table does not know), and the text of a message is not used at all. Taking part names out of a text was tried first and cannot be made sure of: a name that holds another name, a name that is a word of the message, and the forms a message gives a name. A name told in the line is told by `part_label` in the lists of parts, and nowhere else. Tests build files whose page, master and parts are named in marks and words of every kind, with names that hold one another, that are words of the report, and that `escape_debug` and JSON change, and look for every piece of three characters that only the names have, in every line, with a property test (`apps/diacraft-cli/tests/it/roundtrip.rs`); a unit test looks for each phrase of the table in the writer's source, so that a reworded warning is noticed.

What a clean round trip looks like: `parts.changed` is `["/docProps/app.xml"]` (the writer's name), nothing added or removed, `content_types.equal` and `relationships.equal` true, `model.equal` true, and `changed.shape_count` 0. Anything else is a finding. Open and save unchanged is `roundtrip FILE --out OUT`; open, move one shape and save is the same with `--move`. `run` does the same one step at a time (`--cmd file.open` with `edit_template`, `--cmd shape.move`, `--cmd file.save_as`), but needs the shape's `ID`.

### An OpenDocument drawing (`.odg`, `.otg`)

```
$ diacraft-cli roundtrip drawing.odg --out copy.odg
{"bytes":10422,"bytes_dropped":{"count":0,"reasons":{}},"element_diff":{"parts":[],"parts_changed":0},"file_identical":true,"format":"odg","headers_rewritten":0,"opened":true,"parts":{"identical":9,...},"parts_identical":true,"read":{...},"rows_reordered":false,"saved":true,"saved_read":{...},"sha256":"...","variant":"odg"}
```

A file whose extension is `.odg`, `.otg` or `.fodg`, or whose bytes say they are an OpenDocument drawing or drawing template (the manifest's root row, else `mimetype`), goes another way: `roundtrip` and `compare` work on the package, and the drawing is not opened by the engine for them (`run` and `mcp` open and save an `.odg` through `file.open` and `file.save`: see the paragraph "An OpenDocument drawing" above). The package is read (`diacraft_odg::Package`), written again with **every entry copied as it is** (its compressed bytes, method, flags, extra fields, dates and attributes, in the order the entries lie in the file: what an unedited save is), and the two packages are compared part by part and, for the XML parts, element by element. `--move` cannot be honoured yet: the line says `"moved": {"error": "not_supported"}` and the unedited round trip is made all the same. `--out` must have the variant's own extension (`.odg` for a drawing, `.otg` for a template); another is `"save_error": {"error": "other_type", "want": ...}` and nothing is written.

| Field | What it says |
|---|---|
| `format`, `variant` | `odg`, and `odg` or `otg` (`null` when the package does not say what it is). |
| `opened`, `refusal` | `false` with `refusal.class` when the package is refused: `not_zip`, `missing_manifest`, `manifest_malformed`, `not_a_drawing`, `encrypted_package` (password-protected drawings are refused in v0.1), the container's classes. The status is 0. |
| `read`, `saved_read` | The package read and the one written: how many `files`, `folders` and `manifest_rows`, whether `mimetype_regular` (first, stored, no extra field), `signed`, `has_macros`, and the class of each kind of warning (`mimetype_irregular`, `manifest_incomplete`, `manifest_lists_missing_file`, ...). |
| `saved`, `save_error`, `out_bytes` | Whether `--out` was written, else why, by a word. |
| `file_identical` | The file written is the file read, byte for byte. The copy writes a well-formed archive, so it is not always: it is false exactly when `bytes_dropped.count` is above 0, `headers_rewritten` is above 0 or `rows_reordered` is true, and those say why. Headers that agree are not enough: a file can have every entry's headers agree and still hold bytes no header announces. |
| `bytes_dropped` | The bytes of the file read that lie outside every entry the copy writes, and that the copy leaves out: their `count`, and under `reasons` the bytes of each reason that has any, by a fixed word. `unannounced`: bytes no header announces, before the first entry, between two entries, after the last, or between the directory and the end records (one producer writes a data descriptor after entries whose flags announce none: those bytes). `descriptor_form`: a data descriptor of another form than its entry's local header announces (sizes of 8 bytes when the local header holds a ZIP64 record, of 4 when it does not), where flag bit 3 announces one and the announced form is not there. `directory_tail`: bytes of the directory after its last row (a digital signature record, or anything that is not a row). `zip64_end_records`: the ZIP64 end record and its locator (the copy's plain end record counts what they did). `archive_comment`: the archive's comment. `after_end_record`: bytes after the end record. `{"count": 0, "reasons": {}}` when nothing is left out. What this does not count: a data descriptor of the form the local header announces, which an entry copied under headers made anew from its row (see `headers_rewritten`) loses with its old headers, its data being the same compressed bytes. |
| `headers_rewritten` | Entries copied under headers written anew from their rows (the central header wins): a local header that disagrees with its row (its name, method, flags, checksum or sizes, or the sizes its ZIP64 record gives), a row that leaves the local header's offset to a ZIP64 field, or flag bit 3 with no data descriptor of the form its local header announces (sizes of 8 bytes when the local header holds a ZIP64 record, of 4 when it does not). Such an entry's data is the same compressed bytes, and its descriptor goes with its old headers. 0 when every entry keeps its headers byte for byte. |
| `rows_reordered` | The central directory lists the entries in another order than they lie in the file; the copy lists them as they lie. |
| `parts_identical` | Every file of the one package is a file of the other with the same bytes. |
| `parts` | `identical` (how many), and the parts whose trees differ (`element_changed`), that differ in bytes only (`bytes_only`), that do not parse (`unparsed`, with the XML class), that were `added` or `removed`, whose compression method changed (`methods_changed`), whose headers say something else (`headers_changed`, each with its fields: `version_made_by`, `version_needed`, `flags`, `time`, `date`, `attributes`, `extra`, `local_extra`, `comment`), and `order_changed`. At most 50 of each, with counts. |
| `element_diff` | For each XML part whose tree differs: the `counts` of elements `added`, `removed`, `changed`, `moved` and changed in `whitespace` only, and their paths (`/office:document-content/office:body/office:drawing/draw:page[2]/draw:custom-shape[7]`), each change with the names of its `attributes` and whether its `text` changed. Empty in a clean round trip. |

What may cross: **no name a person chose**. A part is named only when the format fixes its name (`mimetype`, `content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, `manifest.rdf`, `META-INF/manifest.xml`, `META-INF/documentsignatures.xml`, `Thumbnails/thumbnail.png`, `Configurations2/`), and is otherwise told by its role: `picture` (under `Pictures/`), `object` (`Object …/`, `ObjectReplacements/`), `macro` (`Basic/`, `Scripts/`), `signature` (another signature file under `META-INF/`) or `a part`. An element or an attribute is named only from a fixed vocabulary, resolved by its namespace URI and never by the prefix the file wrote: the names the project's ODF specs record, under each ODF namespace's own prefix (`draw:custom-shape`, `svg:x`; `<d:rect>` with `d` bound to the drawing namespace shows as `draw:rect`). Anything else, `<draw:MyPassword>` included, is `(unlisted)`. A place among siblings is a number. No text, value or message is in the line.

**A flat drawing** (`.fodg`) is told by its content (an XML document whose root is the office namespace's `document`), not by its name, and is read with the `odg` crate's own reader: the line has `format: "odg"`, `form: "flat"`, `opened`, `variant` and `read` (`reads_back`, `form`, `pages`, `has_macros`, `linked_pictures`, `warning_count` and the class of each kind of warning), and then the file written again through the flat writer with nothing edited, which is the file's own bytes: `saved: true`, `file_identical` (true), `out_bytes` and `saved_read`. `--out` must be a `.fodg`: another name is `"save_error": {"error": "other_type", "want": "fodg"}` and nothing is written, since a flat file is not turned into a package. `--move` says `{"error": "not_supported"}` as it does for a package, and the unedited round trip is made all the same. A flat file that is refused is a report with `opened: false` and the class of the refusal (`xml_entity_refused` for a DOCTYPE, `xml_too_complex`, `xml_malformed` and the rest of the reader's), where a package reader said `not_zip`; no word of the file is in the line.

A clean round trip of an `.odg`: `parts_identical` and `file_identical` true, `element_diff.parts_changed` 0, `saved_read` as `read`. A round trip whose `file_identical` is false with `parts_identical` true, and one of `bytes_dropped`, `headers_rewritten` and `rows_reordered` saying why, is the copy mending the archive and not a finding; `file_identical` false with none of the three saying anything would be one. To see which elements an edit of the drawing changed, open the `.odg` with `run`, edit and save it, and compare the two files with `compare`.

### `--via diacraft`: the trip through `.diacraft`

```
$ diacraft-cli roundtrip drawing.vsdx --out visit.vsdx --via diacraft --edit retext
{"content":{"class":null,"compared":true,"count":2,"differences":[{"found":"text","part":"/docProps/core.xml","path":"/coreProperties[0]/modified[2]/#text[0]"},{"found":"element","part":"/…/pages/page1.xml","path":"/PageContents[0]/Shapes[0]/Shape[0]/Text[0]"}],"same":false},"diacraft":{"bytes":6287},"edit":{"kind":"retext","page":1,"shape":1},"format":"vsdx","input":{"bytes":7111,"sha256":"..."},"model":{"differ":[],"differ_count":0,"same":true},"opened":true,"output":{"bytes":7129,"sha256":"..."},"refusal":null,"variant":"vsdx","via":"diacraft"}
```

The same file with no edit: `"content":{"class":null,"compared":true,"count":0,"differences":[],"same":true}`.

The instrument of the visits of many files (the native format design's section 10). A VSDX package or an OpenDocument drawing (`.odg`, `.fodg`) makes the trip that section 5 is about, **in the engine and not beside it**, in two sessions so that nothing the first one kept reaches the second:

1. `file.open` the input (a template as the file itself, `edit_template: true`);
2. with `--edit KIND` or `--move DX,DY`, the salted edit of the table below;
3. `file.save_as` a `.diacraft` in a scratch folder of this run (a stencil as `.diacraft-stencil`, a template as `.diacraft-template`, any other as `.diacraft`), which is removed when the run ends, whatever ended it: a folder of the system's temporary place (`TMPDIR`) that was not there before, with a mode only the run can use;
4. `file.open` that file in a new session;
5. `file.save_as` the output, `--out`, in the format the input came in, and the **content check** (the design's 4.3; `diacraft_vsdx::compare_content` for a VSDX package and `diacraft_odg::compare_content` for an `.odg`, a flat file compared with a package once it is split the way the reader splits it) of the input and the output.

`--out` must end in a type of the input's format (`.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx` or `.vstm` for a VSDX package; `.odg` for an OpenDocument drawing, `.otg` for an OpenDocument template, which the trip saves back as a template: the kind comes from the name, and a template written under an `.odg` name would be a drawing, which the content check counts as a difference no save made), and is never the input: a name of another format, a `.diacraft` input (it has made the trip) and the input itself as the output are status 1 with words, before anything is made. `--edit` is only for `--via`, and `--edit` with `--move` is status 2 (`--move DX,DY` with `--via` is the move edit with a delta of your own).

**The salted edits** (plan ruling NP13). Each acts on the first top-level shape of the first page that has one, as `--move` picks it, by one engine command with fixed numbers, so the same file gets the same edit on every run and machine:

| `--edit` | Command |
|---|---|
| `cell` | `shape.format {weight: 1.23}` |
| `move` | `shape.move {dx: 0.125, dy: 0.125}` |
| `retext` | `shape.set_text {text: "salt"}` |
| `delete` | `shape.delete`, which takes along the connectors glued to the shape |
| `add` | `shape.drop {master: "rectangle"}` at the centre of the page (of the first page that has a shape, else the first page) |

**The line is one shape every time.** It is the plan's `{via, input, diacraft, output, edit, content, model}` and the fields a reader needs to tell a trip that finished from one that did not:

| Field | What it says |
|---|---|
| `via`, `format` | `diacraft`, and `vsdx` or `odg`: the format the input came in. |
| `input` | `{sha256, bytes}` of the file read. |
| `opened`, `variant` | Whether the engine opened the input, and its type as `file.status` says (`vsdx`, `vssx`, `vstx`, `odg`...; `null` before it opened). |
| `edit` | `null` with no edit. Else `{kind, page, shape}` (a `move` also has `dx`, `dy`; an `add` names the new shape's `ID`), or `{kind, error}` when no edit could be made (`no_shape`: the file has no shape on any page to edit, for every kind but `add`) or `{kind, page, shape, error}` with the kind of engine error. |
| `diacraft` | `null` until the `.diacraft` file was made, then `{bytes}`: its size. |
| `output` | `null` until the output was made, then `{sha256, bytes}`. |
| `content` | `{compared, same, count, differences, class}`. **`same` is `true` or `false` only when the check was made** (`compared: true`); `count` is exact and `differences` are the first 200, each `{part, path, found}`: the part (a name that is package structure, else its role: `a part`, `picture`...), a path of element names and positions from the root (`/PageContents[0]/Shapes[0]/Shape[0]/Cell#1/@V`: no value and no name a person chose) and the kind (`missing_part`, `extra_part`, `bytes`, `element`, `attribute`, `value`, `text`, `instruction`, `order`). When the check was not made, `compared` is `false`, `same` and `count` are `null` and `differences` is empty; `class` is then the class of the check's own refusal (`xml_too_complex`, `package_too_large`...) or `null` when a leg ended the trip before it. |
| `model` | `{same}`: whether the file written reads back as the drawing that was saved as `.diacraft` (the first session's model, after the edit, but for what a save writes of its own: the relationship ids and part names, and the file's bookkeeping in the slot of a page or master that held none, since a master a new shape brought has no part until a file is written; a slot that held something must come back equal), with `differ` and `differ_count` (`page 1`, `styles`...); `same` is `null` when the trip did not get so far, and `class` says why when the file written cannot be read. |
| `refusal` | `null` when every leg was made. Else `{leg, error, problem, words}`: the leg the engine refused or could not make (`open`, `edit`, `save_diacraft`, `open_diacraft`, `save_back`), the kind of engine error (`cannot_open`, `failed`, `bad_params`, `disabled`, `no_shape`...), the kind of trouble opening was (`problem`, for `cannot_open`) and the engine's own words, which are the words `file.open` and `file.save_as` give a person: they carry no text of the file and no path, and a bug (`internal`) has none. |

**A trip that did not finish never says same.** A leg the engine refuses ends the visit with `refusal` in the line, status 0, the legs before it made (`diacraft` is there when the `.diacraft` file was) and `content.same` and `model.same` `null`: a harness that counts `same` true counts it as a failure, and one that counts `same` false does not count it as a difference. **Today** a VSDX package makes the whole trip, a stencil and a template in their own `.diacraft` types (a `.diacraft` drawing is saved as a VSDX type from the model alone), and an `.odg` drawing makes the whole trip too (the first leg, `save_diacraft`, and the last, `save_back`, wired the `.odg` model writer), the content check of its input and output being `diacraft_odg::compare_content`: a hand-built `.odg` of another program is not the same content, where the model writer writes what the other program wrote otherwise (the check counts the version attributes, the generator, an empty settings part and the names of automatic styles as the same content; a shape is written as it was read), and the differences are listed. A file that does not open is a report with `refusal.leg: "open"` and the engine's words; a scratch folder that cannot be made, a file written that cannot be read back and a mistake in the line are status 1 (messages on standard error) or 2.

What may cross to a person who must not see the file is what the plain round trip lets cross (the hash, the sizes, the counts, a part's name when it is package structure, and the check's paths and kinds), and the engine's words of a refusal; the content of the file, a formula, a value, a text and a name a person made up are not in the line (`apps/diacraft-cli/tests/it/roundtrip_diacraft.rs` looks for the names of the files it builds in every line).

## `compare`

```
$ diacraft-cli compare before.odg after.odg
{"element_diff":{"parts":[{"changed":[{"attribute_count":1,"attributes":["svg:x"],"path":"/office:document-content/office:body/office:drawing/draw:page/draw:rect[2]","reordered":false,"text":false}],"counts":{...},"part":"content.xml",...}],"parts_changed":1},"first":{"opened":true,"read":{...},"sha256":"...",...},"format":"odg","parts":{...},"parts_identical":false,"second":{...}}
```

Compares two OpenDocument drawings, so that a harness can plant a difference of its own, an attribute changed or an element moved, and see it named. Each file is read as `roundtrip` reads it (`first` and `second`: `sha256`, `bytes`, `opened` or `refusal.class`, `variant`, `read`). When both open they are compared once (`diff_packages`): `parts_identical`, `parts` and `element_diff`, exactly as in the round trip's line above, with its rules for what may cross. A flat drawing (`.fodg`) is read in the same way (`first` or `second` has `form: "flat"`), and it has no parts: with a flat drawing on either side `parts` is `{"comparable": false, "class": "flat_not_compared"}` and there is no `parts_identical` and no `element_diff`. A file that does not open is a report with its class and no comparison, and the exit status is 0; a file that cannot be read from disk is status 1; anything but two files on the line is status 2. Nothing is written.

### `compare --content`: the same content

```
$ diacraft-cli compare --content before.vsdx after.vsdx
{"content":{"class":null,"compared":true,"count":3,"differences":[{"found":"value","part":"/…/pages/page1.xml","path":"/PageContents[0]/Shapes[0]/Shape[0]/Cell#0/@V"},...],"same":false},"first":{"bytes":7111,"sha256":"..."},"format":"vsdx","second":{"bytes":7110,"sha256":"..."}}
```

The content check of the design's section 4.3 on two files of one format, a VSDX package (any of the six types; part names compared as OPC compares them, spacing, attribute order, prefixes, comments and the way the zip is made ignored, a sheet's cells as a set by name) or an OpenDocument drawing (a flat one compared with a package). For a VSDX package three things say nothing and are the same (`docs/diacraft-format.md` 4.4): an empty `Colors`, `FaceNames`, `StyleSheets`, `DocumentSheet`, `Connects` or `Shapes` and an absent one, each only under its own parent (the document part's root, a page's or a master's root, a group's `Shape`), a relationships part on one side only that holds no row and none, and a shape's `Section`, `Text` and `ForeignData` children in any order among themselves (the sections keep their own order); a container or a relationships part that holds anything, an empty container under another parent, a part of any other name whatever its root holds, and every other child's place, still differ. Three more say nothing (`docs/diacraft-format.md` 4.4): **a relationship row that the format implies and the other package lacks**, a page's `master` row to the part of a master that a shape of the page names by its `Master` and its `page` row to the part of its `BackPage`, when the other package has no row of that type to that part and none of that `Id`; **a part's content type given where it had none** (a part with no type in one package and one in the other, a property part typed as the drawing in one and as its own kind in the other, and an `Override` for a relationships part against the `Default` for `rels`); and **a `Row` with no `IX`** against the same row whose `IX` is the count of the rows before it in its section (the rows of a section keyed by name keep an absent `IX` absent). A row for a master no shape uses, a row with another target, another `Id` or another type, two different types for a part that has one in both, a `Default` against an `Override` for a part typed in both, a row for no part, an `IX` that disagrees with the row's place, and a part that holds only an empty root of another name (a dropped unknown part) still differ. For an OpenDocument package these say nothing and are the same (`docs/diacraft-format.md` 4.5): **`office:version` and `manifest:version`**, wherever they stand; **an empty `settings.xml` or `meta.xml`** and none (a `meta.xml` that holds only a generator is one), and an empty `office:settings` under the root of a flat file and none; **the generator of `office:meta` and the order of its fields** (the copies of one field in the order written); **the manifest's rows in any order** and **a row that only says what an entry says** (a row for an entry only its package holds, a plain folder row that files stand under, and the row of the folder `DiaCraft/`); and **the names of automatic styles**: two automatic styles are the same when their content is (copies of one style count as one, and a reference from one automatic style to another is resolved to the content, a circle and a chain of more than 64 compared as written), a shape that names one must name its match, a style whose property changed is one difference in that style, and a common style's name is kept. **DiaCraft's own carrier counts as nothing when only the second file has it**: its entries under `DiaCraft/`, their manifest rows, and its attributes and elements; what the first file held is compared as written, so **the first file is the one the drawing came from** and `compare --content B A` need not give the count of `compare --content A B` for a file that has such markup. `meta:user-defined` fields added, `dc:creator` or `meta:initial-creator` taken out, `meta:creation-date` added, `meta:print-date` taken out, a layer set added where the source had none, a folder row typed as a kind, DiaCraft's own markup and every other difference still count. It does not open them as drawings: the check reads the packages. The line is `{format, first: {sha256, bytes}, second: {sha256, bytes}, content}` with `content` as the trip's line above: `same`, the exact `count` and the first 200 `differences` by part, path and kind. A file that is not a package is a report with `content.compared: false` and the reader's class, status 0. **Two files of different formats** (`vsdx` and `odg`) are refused in words, status 1 and no line, and so is **a `.diacraft` file**, which has no content check of its own (its content is the model: compare the files it came from, or the files a trip through it wrote); a file that cannot be read from disk is status 1; anything but two files and `--content` once is status 2. **One place is one difference, however much it differs**: a value that differs, an attribute or an element on one side only and a part on one side only are each one difference, so a change planted on a place the check already counts against the source (a value the trip wrote otherwise, inside an element or a style only one file has) does not raise the count, and against the file it was planted in it is one. `compare` without `--content` is as it was. **The children of an element that are not keyed are set side by side by what they are, and not by place** (`docs/diacraft-format.md` 4.3): an element the second file has and the first lacks, or the other way, is one difference and every sibling after it is still compared in depth with its own (a difference planted deep inside a later sibling is counted and found where it is); the same children in another order are one `order` and what is inside them is still found; and a manifest that begins with a DOCTYPE that holds only an external id (an old OpenOffice.org writer's) is read as one that has none.

## The log

Log records are written to standard error, one line each. `RUST_LOG` sets the level: `off`, `error`, `warn` (the default),
`info`, `debug` or `trace`; anything else is reported and `warn` is used. Text a client sent that a record repeats is
escaped first, so that a control character in it does not reach a terminal.

## The settings

`run` and `mcp` (a session of this process: `mcp --headless`, and `mcp` when it fell back to a headless session; not `--connect`, which drives the window's session and its settings) open the person's settings, `config.toml` and the dictionaries and AutoCorrect lists beside it, as the window does, and save them when the command ends (`diacraft_engine::prefs`; `docs/development.md`, "The person's settings"). They share one file with the window: the command line holds the engine's sections (`[proofing]`, `[autocorrect]`, `[language]`) and keeps the window's own keys, and a key it does not know, as they are. The other commands (`commands`, `oracle`, `export`, `convert`, `roundtrip`, `compare`) read and write no settings.

- `DIACRAFT_NO_PREFS` set to anything, the empty text too: nothing is read or written; what a run changes is kept in memory until it ends. **A script or an agent that must not touch the person's settings sets it.**
- `DIACRAFT_CONFIG_DIR`: the folder of the settings in place of the platform's (`~/.config/diacraft` on Linux, `~/Library/Application Support/diacraft` on macOS, `%APPDATA%\diacraft` on Windows). Set but empty it does what `DIACRAFT_NO_PREFS` does.
- What is wrong with the settings (a file that is broken, too big, not text or from a newer build, a folder that cannot be written) is said once on standard error, one `diacraft-cli: ...` line each, and the command goes on with the defaults or the file's values. A file that cannot be saved is said at the end. Standard output is not touched.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-28 | trivial | The app's description in the command line takes `--reset-graphics` (added the option to the app, and the test that holds the description to the app's help failed), and "Shell completions" names it in the list of the app's options |
| 2026-11-27 | minor | The salted `delete` edit is `shape.delete`, which takes the connectors glued to the shape along |
| 2026-11-26 | minor | The section "The settings": `run` and `mcp` (a session of their own) open and save the person's settings, `DIACRAFT_NO_PREFS` and `DIACRAFT_CONFIG_DIR` isolate or move them, what is wrong is said once on standard error |
| 2026-11-26 | minor | "The page's routing settings and a connector's route" is followed by an example of `connector.reverse_ends` (real lines: the ends swapped, a box left alone and listed, the route read back from the other end) |
| 2026-11-26 | minor | Issue 10 of the demo video: `convert`'s `warnings` are objects `{class, words}` and not the bare class words, with a class for every kind of warning a save gives (a convert of a drawing the app made to `.odg` answered eight, all `other`), and the words say what changed and where (an array of strings before: a script that read each warning as a word reads `warnings[].class` now); `roundtrip` tells the same classes, as words only |
| 2026-11-26 | minor | `shape.connection_points`, which doubles and halves the connection points of a shape's edges as one undo step, with a run of it and its refusal when nothing is left to halve ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)") |
| 2026-11-26 | minor | `--version` and `--help` are looked for before a `--` only, so that after it they are the names of files (`export --out p.svg -- --help` reads a drawing named `--help`; both used to print the page or the version) |
| 2026-11-26 | minor | The refusal of an `.odg` save for a group with no name that holds data is one of the refusals that do not end with the undo |
| 2026-11-26 | minor | Round 2: `export` and `convert` to a picture take the first page that is not a background page when `--page` is left out (meeting the table of words), and the packages' sentences name the `.rpm` beside the `.deb` (its zsh completions are in Fedora's folder) |
| 2026-11-26 | minor | `mcp` and the app take `--automation-read-root` and `--automation-write-root` on the clap-read command line (meeting): the two are options given again for more folders, named as folders in the completions of `mcp` and of the app's script, with the words `needs a value` for a flag with no folder |
| 2026-11-26 | minor | The command line is read by clap (`apps/diacraft-cli/src/cli.rs`, the one description of its commands and options), with the strictness and the words it had (`unknown option`, `was given twice`, `needs a value`); `--` ends the options for every command and a word with one dash is an option the command does not have (it was a file name); `--color auto\|always\|never` and `NO_COLOR`, `CLICOLOR_FORCE`, `CLICOLOR` and `TERM` colour the help and the words of a failure at a terminal and nowhere else (never the JSON, the MCP stream or the exit statuses), and the help is wrapped to the terminal's width; the help page comes from one table of its words (`src/help/text.rs`), as written for a pipe, byte for byte the page it was before the two new commands, `--color` and the colour variables; `completions SHELL [--program NAME]` writes the completion script of bash, zsh, fish, PowerShell or Elvish (commands, options, values, the command ids of `run --cmd`, files by their type) for the command line or the app; `man [--list \| PAGE]` writes the manual pages; the `.deb` installs both |
| 2026-11-26 | minor | The SVG bullet of `export` says a shape is a group with its alt text as `<title>` and `<desc>` (its name as the title when it has none), and the page's alt text is the file's |
| 2026-11-26 | minor | Follow-up: `export` and `convert` to a picture draw the first page that is not a background when `--page` is left out (it drew the page in the first place of the drawing, a background page in a file that lists them first) |
| 2026-11-26 | minor | A paragraph on `shape.offset` and `shape.fit_curve` (the numbers are in the cells' own units, the answer's `changed` and `not_fitted`, one undo step, a Fit Curve that changes nothing is none) with the steps that draw an offset |
| 2026-11-26 | minor | `shape.connection_edges`, a query of the edges of a shape's outline, the connection points on each and what a doubling of each would make under a minimum distance between points ([Editing commands](editing-commands.md), "Connection points: double and halve (CPDOUBLE)") |
| 2026-11-26 | minor | `mcp` takes `--automation-read-root DIR` and `--automation-write-root DIR` (and the variables `DIACRAFT_AUTOMATION_READ_ROOT` and `DIACRAFT_AUTOMATION_WRITE_ROOT`); a folder that is not there and `--connect` with a root flag are status 2, the default mode stops when the app answers; the other commands have no roots |
| 2026-11-26 | minor | `run` reaches the ShapeSheet's writes, `sheet.set_cell` and `sheet.set_cells`, with an example |
| 2026-11-26 | minor | `run` reaches the ShapeSheet's rows and sections, `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section`, with an example and the refusal of an insert a master numbers |
| 2026-11-26 | minor | Follow-up: the example of a middle insert into a Process shape's Geometry takes a copy of its rows first (`renumbered.copied`), and the refusal it quotes is |
| 2026-11-25 | minor | `run` reaches the ShapeSheet's queries, `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate`, with an example |
| 2026-11-25 | minor | `run` has an example of the page's routing settings (`page.routing`, `page.set_routing`) and a connector's route (`connector.path`) |
| 2026-11-25 | minor | `run` reaches `file.check_compatibility`: what a save would carry and lose, answered with no file written |
| 2026-11-25 | minor | `compare --content` sets the children of an element that are not keyed side by side by what they are, so that an element put in or taken out is one difference and every sibling after it is compared in depth with its own; a manifest that begins with a DOCTYPE that holds only an external id is read as one that has none; `roundtrip --via diacraft` of an `.otg` writes an `.otg` (an `.odg` name is refused for a template, and `.otg` for a drawing), so that the kind is no difference |
| 2026-11-25 | trivial | The `mcp` section points to the MCP bundles of a release |
| 2026-11-23 | trivial | Sentences reworded so that each reads whole on its own; nothing they say changes |
| 2026-11-23 | minor | `app.version`'s `git` is the build line (the plain version for a build of its tag, `<version>-dev+<commit>` before the tag exists), and the version is 0.2.0 |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | The commands that read a file are `file.open` (every type it reads), `file.new {template}`, `stencil.open` and `insert.picture` (it said only `file.open`, of the VSDX types), a drawing opened from a VSDX file is saved as an `.odg` and one from an `.odg` as a VSDX type (it said not yet), an `.otg` opens (it said not yet), the open of an `.odg` names what the drawing does not hold, what an `.odg` cannot say rides inside it and only a background page is refused, Save As writes a `.fodg` drawing as another type, the `.odg` save's answer has `kind`, `form`, `carried` and `checked`, and the trip of a hand-built `.odg` is judged as the content check does and the two `convert` examples of a drawing type show `carried` and `checked`, which the line holds |
| 2026-11-23 | minor | `convert`'s `checked` covers a drawing made from a template file of another format. |
| 2026-11-22 | minor | `compare --content` says one place is one difference however much it differs: a change planted where the check already counts a difference does not raise the count. |
| 2026-11-22 | minor | Second round: `compare --content` of two `.odg` files is oriented (the first file is the source): DiaCraft's own carrier (its entries under `DiaCraft/`, their rows, its attributes and elements) counts as nothing when only the second file has it; copies of an automatic style count as one; a reference from one automatic style to another is resolved; the table of references follows the ODF 1.4 schema. A trip whose only difference is the carrier is the same content through `roundtrip --via diacraft` and `compare --content`. |
| 2026-11-22 | minor | `convert`'s line has `checked`: the file read back and compared with the drawing after a save through a model writer, class by class. |
| 2026-11-22 | minor | For two `.odg` files `compare --content` (and the trip's `content`) counts as the same content `office:version` and `manifest:version`, the generator of `office:meta` and the order of its fields, an empty `settings.xml` or `meta.xml` and an empty `office:settings` under a flat root, the manifest's row order and its rows for entries the writer wrote, and the names of automatic styles; the command line's test drawing, a hand-built `.odg` of another program, is the same content after the trip through `.diacraft` (it differed in the versions, an empty settings part and the names of its styles). |
| 2026-11-22 | minor | `file.save_as` is available for a drawing opened from a flat `.fodg` (it was disabled with a reason), and writes it from the model as a `.diacraft` file, an `.odg` package, an `.otg` or a VSDX type, so `run` and `mcp` save it as any of them and `convert` converts it to every drawing type; a `.fodg` is still no Save As target (refused in words that point to the `.odg`), and `file.save` still writes the flat file. |
| 2026-11-22 | minor | `convert` writes the crossings between the VSDX and OpenDocument formats, takes `--drop-macros` and `--drop-objects` (a mistake for a picture, each once), and its line has `carried`. |
| 2026-11-22 | minor | `convert` writes an `.odg` and an `.otg` from a `.diacraft` drawing (the types are the engine's own list), `run` saves a `.diacraft` drawing or a new one as an `.odg` or an `.otg` from the drawing alone and opens an `.otg` as a new drawing, and `roundtrip --via diacraft` of an `.odg` makes the whole trip and says what the content check finds |
| 2026-11-22 | minor | `app.version` answers `{app, version, git, year}`: the git release and the year of the commit the binary was built from |
| 2026-11-21 | minor | The `.odg` save's `warnings` say two kinds of loss, and `connector.add` and `connector.style` carry none for a curved connector |
| 2026-11-22 | minor | The `convert` command: a drawing as another type by the extension of `--out`; a drawing type goes through the engine's `file.open` and `file.save_as` (a refusal passes through in the engine's words, and `--out` is never the input, a link to it included), a picture type through `export`'s path and options; the line holds `in`, `out`, `format`, `bytes` and, for a drawing, the save's counts and warnings by class |
| 2026-11-21 | minor | For a VSDX package `compare --content` (and the trip's `content`) counts a part of any name but a relationships part's as a difference whatever its root holds, and an empty `Colors`, `FaceNames`, `StyleSheets`, `DocumentSheet`, `Connects` or `Shapes` as an absent one only under its own parent |
| 2026-11-21 | minor | For a VSDX package the content check (`compare --content`, and the trip's `content`) also counts as the same a relationship row that implies and the other package lacks (a page's `master` row to a master its shapes use, its `page` row to its `BackPage`), a part's content type given where it had none (a property part typed as the drawing and given its own, an `Override` for a relationships part against the `Default` for `rels`), and a `Row` with no `IX` against one whose `IX` is the count of the rows before it |
| 2026-11-21 | minor | `run --cmd file.new --cmd file.save_as` writes `.diacraft` from a new drawing's model and a VSDX type from the model alone (a new drawing keeps no file's bytes); a drawing opened from a `.vsdx` is saved as VSDX, silently |
| 2026-11-21 | minor | For a VSDX package the content check (`compare --content`, and the trip's `content`) counts an empty `Colors`, `FaceNames`, `StyleSheets`, `DocumentSheet`, `Connects` or `Shapes` and an absent one, a relationships part on one side only that holds no row and none, and a shape's sections, text and foreign data in any order among themselves as the same |
| 2026-11-21 | minor | A `.diacraft` drawing saved as a VSDX type keeps the thumbnail, `Application` and `modified` of the VSDX file it was read from while it is that drawing; one made from a template has no such file and is written with a new file's, and a template opened with `edit_template` keeps its own |
| 2026-11-21 | minor | `file.print {color}`: `grayscale` writes the print in black and white |
| 2026-11-21 | minor | `run --cmd file.save_as` of a drawing opened from an `.odg` writes a `.diacraft` file (the refusal is lifted: the model holds what the `.odg` holds), and `roundtrip --via diacraft` of an `.odg` makes its first leg and is refused at the last, the save back as an `.odg` |
| 2026-11-21 | minor | `roundtrip --via diacraft [--edit KIND]` (the file is opened, edited if asked, saved as `.diacraft` in a scratch folder, opened in a new session and saved back in its own format, and the line reports the content check of the file and what came back: `content {compared, same, count, differences}`, `model {same}`, `diacraft {bytes}`, `output {sha256, bytes}`, and `refusal {leg, error, problem, words}` for a leg the engine refuses, which never says same; a VSDX package makes the whole trip and an `.odg` is refused at the first leg) and `compare --content FIRST SECOND` (the content check on two files of one format, a VSDX package or an `.odg`; two formats and a `.diacraft` file are refused in words) |
| 2026-11-21 | minor | The `.diacraft` paragraph says a `.diacraft` drawing is saved as a VSDX type, from the drawing alone, and is not yet saved as an `.odg` |
| 2026-11-21 | minor | `run --cmd file.open` reads a `.diacraft` file by its bytes and `--cmd file.save_as` and `--cmd file.save` write one (a save across the formats that is not written yet is refused in words naming the way out), and `export` reads a `.diacraft` file as `file.open` does |
| 2026-11-21 | minor | The `problem` row says `too_big` beside what is held is told by how far the refused read got, in its XML and not its pictures, and only when that is clearly past the budget. |
| 2026-11-21 | minor | The `problem` words of a round trip's refusal name `out_of_memory`, and say what it is and what `too_big` is beside it; the read of the saved file holds the memory it may use while it runs |
| 2026-11-20 | minor | The status line's date is that of the newest row, 2026-11-20 (it said 2026-11-19) |
| 2026-11-20 | minor | A hit on a line along no axis has a `normal` and its mark `axis` null, a resize of a turned or flipped shape tells `frame`, `corner_boxes` is also told by `shape.place` and `snap.at`, and `snap.at` refuses `connect` beside a box, `edges`, `nudge`, `keep` and `rotate` |
| 2026-11-19 | minor | A command takes only the words of `snap` it has, and `snap.at` takes `edges`, `keep`, `lock`, `nudge`, `view` and `rotate` |
| 2026-11-19 | minor | Third part: the `snap` result of `shape.move` carries `corner_boxes`, how many moving shapes lined up by the box round their corners |
| 2026-11-19 | minor | `file.save_as` is not available for a drawing opened from a flat file (disabled with the reason: `command_list` shows it and a run that asks for it ends in words), and `file.open`'s `warnings` are at most 20 with the notes of the open itself (a flat file's, the extension's, the stencil names') first and last and never cut |
| 2026-11-19 | minor | `commands` names the words of `snap` (`lock`, `nudge`, `view`, `keep`, `radius`) and `marks` |
| 2026-11-18 | minor | `run`, `export` and `mcp` open a flat OpenDocument drawing (`.fodg`) by what the file is, `file.save` writes it back as a flat file in place and a `file.save_as` of it is refused in words, `roundtrip` of one reads it and writes it again unedited to a `.fodg`, and `compare` says `flat_not_compared` |
| 2026-11-09 | minor | `run` opens, edits and saves an `.odg` through `file.open`, `file.save` and `file.save_as` (an `.odg` is saved as an `.odg` and a VSDX file never as one), a change the `.odg` writer does not write is a refused save in the writer's own words (the step is named on standard error and nothing is written), `warnings` say what a new shape has that the file does not carry, and `export` reads an `.odg` |
| 2026-11-08 | minor | The `.odg` round trip's line gives `bytes_dropped` (a count, and the bytes of each reason by a fixed word: `unannounced`, `directory_tail`, `zip64_end_records`, `archive_comment`, `after_end_record`), `headers_rewritten` and `rows_reordered`, so that `file_identical` false always has a stated cause; the claim that `file_identical` holds when every entry's headers agree is gone; a copied data descriptor keeps the form its local header announces |
| 2026-11-08 | minor | The `compare FIRST SECOND` command (one comparison of two `.odg` files, with the round trip's rules for what may cross); the `.odg` line shows a part by its role unless the format fixes its name, and an element or attribute only from a fixed vocabulary by namespace URI, `(unlisted)` otherwise; `parts.headers_changed` |
| 2026-11-08 | minor | `roundtrip` takes an OpenDocument drawing (`.odg`, `.otg`, or bytes that say they are one): before its reader exists it reads the package, copies every entry as it is, and reports `parts_identical`, `file_identical` and `element_diff` (elements added, removed, changed, moved and changed in white space only, by path); `--move` is `not_supported` for it yet, and `--out` keeps the variant's extension |
| 2026-11-07 | minor | The report's schema is 7, so a line of schema 6 is refused by `--aggregate`: the score follows five changed rules (`CEILING`, `FLOOR`, `INT`, `INTUP`, `ROUND`, `TRUNC`, `MODULUS`, `LN`, `LOG10`, `SQRT` and `POW` on the number shown in the argument's unit, the last nine, our rule; the small term of `TEXTHEIGHT` and `TEXTWIDTH`; `THEMEPROP("Embellishment")` 2 where a sheet reads no theme); the reason `stale_theme_property` is gone from `calc.left_out` (those cells are scored); `stale_theme_colour` counts only the colours under a scheme the file no longer selects whose saved value is the file's first theme part's reading and not the sheet's own, the rest of that rule's condition being scored; and `calc.stale_theme_colour_by_function` (with `stale_theme_colour_by_function_unlisted`) tallies those cells by the spec's names of the functions their formulas call |
| 2026-11-07 | minor | The report's schema is 6 (about 418,000 cells that were not calculated are compared, so a line of schema 5 is a score of fewer cells and is refused by `--aggregate`); `oracle` applies the curve rule only to the `C` and `D` of `(Rel)EllipticalArcTo` rows, and the real-file salt excuses a curve agreement only for a circle or angles a half turn apart |
| 2026-11-07 | minor | `oracle` compares the `C` and `D` cells of an elliptical arc (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`) by the ellipse the pair describes as well as by raw value, and reports `calc.agreed_by_curve` |
| 2026-11-07 | minor | `mcp --connect` reads no faces and refuses `--bundled-fonts`; `mcp` in the default mode reads them only once it has chosen a headless session |
| 2026-11-07 | minor | The Fonts section; `mcp`, `run`, `export` and `roundtrip` read the machine's faces once at the start and share them with the engine's commits and the export; `--bundled-fonts` |
| 2026-11-07 | minor | `roundtrip` tells an error by its kind (`error`, `problem`) and a warning by its class, and copies no text of a message, so that no part name and no piece of one reaches the line; the cleaner that took names out of a text is gone |
| 2026-11-07 | minor | `oracle` leaves out a font cell that gives a number other than 0 (`machine_font_number`), a shadow property a theme answered on the no-shadow branch (`theme_no_shadow`) and a cell whose own formula can only fail and saves a value (`formula_must_fail`); the score follows the rules for a group's scheme cells, a row named in another letter case and the table of `THEMEVAL`'s names; the report's schema is 4 |
| 2026-11-07 | minor | `oracle` salts a seeded sample of the real cells that agreed with a wrong saved value and fails the file when one still agrees (`calc.real_salt`, `real_salt_failed`); a cell reading a `Themed` operand or a date past 9999 has its own reason (`themed_operand`, `date_past_9999_operand`) and `unsupported_unlisted` keeps only functions the spec does not list; `THEMEPROP` on a sheet that reads no theme is left out (`stale_theme_property`); the report's schema is 3 |
| 2026-11-07 | minor | The `roundtrip` command; `file.open`'s `edit_template` and a template opening as a new drawing made from it; `file.save_as`'s `drop_macros` and the refusal that asks for it; what a Save As between the six types does |
| 2026-11-01 | minor | `oracle` joins the reader to `calc`'s comparer in the app layer; every run begins with a salted control and has no score when it fails; the report places every cell (compared, left out by reason, not calculated, no formula); `--aggregate` gives totals first, counts a file once, and refuses a line that cannot be true |
| 2026-11-01 | minor | The `export` command: one page of a drawing as PNG, SVG or PDF, by the extension of `--out`; what each holds, what it says it left out, and that the command line has no granted folders |
| 2026-11-01 | minor | `oracle`'s refusal `detail`, the report of a refused file, the container's and XML sizes, `skipped` complete; `file.open` keeps the file and says why it failed in a typed field |
| 2026-11-01 | minor | The `oracle` command, and `file.open` reads a file |
| 2026-11-01 | minor | `file.save`, `file.save_as {path}` and `file.status` write a `.vsdx` and report whether the drawing is unsaved |
| 2026-10-31 | minor | Final review: parsing 16 MiB takes about 1.1 to 1.5 GiB for small objects and up to about 2 GiB for deeply nested ones (the page said about 1.1 GiB), and four lines of 16 MiB kept by a run hold about 5.6 to 8 GiB (the page gave no figure), measured on the release binary |
| 2026-10-31 | minor | The array form of `--commands` takes at most 16 MiB in all and a line of JSON Lines at most 16 MiB, so that the memory of a run is bounded; the usage text for `run` reworded |
| 2026-10-31 | minor | Created: `mcp`, `run` and `commands`, the limits and the exit statuses; first run against the built binary |
