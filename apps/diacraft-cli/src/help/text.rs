//! The words of the help page of `diacraft-cli`, as a table. `help.rs` lays them out.
//!
//! The text of a piece keeps the line breaks the page has always had, written `\n`: they are the page as a pipe gets it. A terminal gets the same words joined and broken again at
//! its width. A key names a piece (`mcp.headless`): the command description reads the sentences by key, and a catalogue of translations is looked up by the English.
//!
//! The list of types of `convert` below names the types of the VSDX family by their product, as the owner allowed on 2026-11-23: the line that holds the list is an exact text of
//! `PRODUCT_USES` in `xtask/src/brands.rs`, and of the export allowlist, so it stays one line of this file.

use super::{Entry, blank, heading, lead, para, row, synopsis, usage, variable};

/// `diacraft-cli --help`.
#[rustfmt::skip]
pub const PAGE: &[Entry] = &[
    usage("usage", "Usage:", "diacraft-cli <command> [options]"),
    blank(),
    heading("commands", "Commands:"),
    synopsis("diacraft-cli mcp [--headless | --connect ADDR] [--automation-read-root DIR]... [--automation-write-root DIR]... [--bundled-fonts]"),
    para("mcp.about", 6, "Run the MCP server on standard input and output (JSON-RPC lines; see docs/mcp.md)."),
    row("mcp.headless", 8, "--headless", 24, "a session of its own, with no window"),
    row("mcp.connect", 8, "--connect ADDR", 24, "the running app's control channel, such as 127.0.0.1:7997"),
    row("mcp.automation-read-root", 8, "--automation-read-root DIR", 24, "keep the files the agent's commands read (a drawing, a stencil, a template or a picture) to this folder\n\
        and the folders in it; give it again for more folders. Links and .. are followed before the check, and a\n\
        refusal says so in words. A session of this process only: refused with --connect (the app keeps to the\n\
        roots it was started with: `diacraft --control <port> --automation-read-root DIR`)"),
    row("mcp.automation-write-root", 8, "--automation-write-root DIR", 24, "the same for the files its commands write (a save, an export, a print). A kind with no folder grants no\n\
        access of its kind: give both. With neither flag (and neither variable below) any path you can reach\n\
        may be read and written, as it has been; give / (and a drive's root on Windows) to say so"),
    row("mcp.bundled-fonts", 8, "--bundled-fonts", 24, "the bundled faces alone, for a session of its own (see Fonts below); refused with --connect"),
    para("mcp.note1", 6, "With neither --headless nor --connect, it looks for the app at 127.0.0.1:7997 once and uses it if DiaCraft answers there;\n\
        if nothing does, or another program does, it runs headless. It never drives another program. With a root flag and the app\n\
        answering, it stops (exit status 1) rather than use an app it cannot keep to the roots: give --headless."),
    para("mcp.note2", 6, "The other commands (run, export, convert, roundtrip, oracle, compare) read and write the paths you type, and have no roots."),
    blank(),
    synopsis("diacraft-cli run [--cmd ID [--params JSON] | --commands FILE|-]... [--bundled-fonts]"),
    para("run.about", 6, "Runs commands one after another in a new headless session, printing a JSON line per command:\n\
        {\"step\": N, \"command\": ID, \"result\": ...}. The first command that fails ends the run: it is named on\n\
        standard error and the exit status is 1. No document is open at the start, so begin with --cmd file.new."),
    row("run.cmd", 8, "--cmd ID", 24, "a command id, such as file.new or page.add"),
    row("run.params", 8, "--params JSON", 24, "a JSON object: the parameters of the --cmd just before it"),
    row("run.commands", 8, "--commands SRC", 24, "a file, or - for standard input, holding steps in one of two forms. A JSON array\n\
        of step objects (16 MiB at most in all), or JSON Lines, a step object on each line\n\
        (16 MiB at most a line). A step object is {\"command\": ID, \"params\": {...}}, and\n\
        \"id\" is accepted in place of \"command\". Every step of every source is read and\n\
        checked before the first one runs."),
    row("run.bundled-fonts", 8, "--bundled-fonts", 24, "the bundled faces alone (see Fonts below)"),
    para("run.note1", 6, "A run holds at most 100000 steps and 64 MiB of --commands text, all sources together."),
    blank(),
    synopsis("diacraft-cli commands"),
    para("commands.about", 6, "Print every command, with its parameter doc and whether it can run, as JSON."),
    blank(),
    synopsis("diacraft-cli oracle FILE... [--json] [--source NAME]"),
    synopsis("diacraft-cli oracle --aggregate REPORTS... [--cap N]"),
    para("oracle.about", 6, "The oracle harness (docs/cli.md): reads each FILE as a drawing package, works its formulas out and compares them with the values and\n\
        errors it saved, never opening it in anything else. It reports its SHA-256, its category, the cell and function names the spec lists and how\n\
        often it uses them, what did not parse, the sizes that confirm the caps, the class of the refusal if it would not open, and the score:\n\
        every cell is compared, left out (with the reason), not calculated, or holds no formula. No formula text and no text content. Every run\n\
        first scores a built document with known-wrong values; when that control fails there is no score and the exit status is 1."),
    row("oracle.json", 8, "--json", 24, "one line of JSON for each file, in place of a few lines of words"),
    row("oracle.source", 8, "--source NAME", 24, "the repository or host the file came from, kept in the report for the aggregation"),
    row("oracle.aggregate", 8, "--aggregate", 24, "the files are lines of reports printed by --json, and the output is their totals (reports, files, refused, scored,\n\
        cells compared, left out and not calculated) and their score by file, by source, and with each source cut to\n\
        --cap files (default 5). A line that cannot be true stops it."),
    para("oracle.note1", 6, "A file that cannot be read from disk is named on standard error and the exit status is 1 once the rest are done; a file that is not a\n\
        package is a report with its class, and the exit status is 0."),
    blank(),
    synopsis("diacraft-cli export FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]"),
    para("export.about", 6, "Writes one page of the drawing FILE as a picture, a vector file or a PDF, by the extension of PATH: .png, .svg or .pdf. Prints one\n\
        JSON line: where it wrote, the page, the size, and what was left out or drawn by a stand-in, as the window counts it."),
    row("export.out", 8, "--out PATH", 24, "where to write (replaced if it is there; never the drawing itself). Written whole or not at all."),
    row("export.page", 8, "--page N", 24, "the page's place in the drawing, from 1 (the default is the first page that is not a background)"),
    row("export.dpi", 8, "--dpi N", 24, "the pixels to the inch of a PNG (96 by default, at most 2400)"),
    row("export.text", 8, "--text MODE", 24, "in an SVG or a PDF, text (the default: letters as text where the font allows, else outlines) or outlines"),
    row("export.transparent", 8, "--transparent", 24, "no white paper under the drawing (a PDF is then written without the white rectangle)"),
    row("export.bundled-fonts", 8, "--bundled-fonts", 24, "the bundled faces alone (see Fonts below)"),
    para("export.note1", 6, "The file is opened and the page drawn as the window draws it; the grid, rulers and guides are not in the picture."),
    blank(),
    synopsis("diacraft-cli convert FILE --out PATH [--drop-macros] [--drop-objects] [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]"),
    para("convert.about", 6, "Converts the drawing, stencil or template FILE to another type, by the extension of PATH (in any case), and prints one JSON line: {in, out,\n\
        format, bytes, ...}: in is the type FILE is (told by its bytes), format the type written.\n\
        The types written are DiaCraft's own .diacraft, .diacraft-stencil and .diacraft-template,\n\
        the Visio types .vsdx, .vsdm, .vssx, .vssm, .vstx and .vstm, the OpenDocument drawing .odg and template .otg, and the pictures .png, .svg and\n\
        .pdf. Any other extension is a mistake in the command line (exit status 2, nothing written).\n\
        A drawing type is a Save As: FILE is opened (a template as the file itself) and saved by the engine's own file.open and file.save_as, so what the\n\
        engine does not write for this drawing is refused in its words (exit status 1, nothing written, a file that was at PATH left as it was): for\n\
        example a stencil is never saved as an .odg, and a macro project is not written into a VSDX type that allows none. A drawing of a VSDX file is\n\
        saved as an .odg or an .otg, and a drawing of an .odg as a VSDX type: what the file written cannot say rides inside it. The line\n\
        of a drawing adds the save's own counts: variant, macros_dropped, parts (kept, rewritten, added, removed: a VSDX save), copied (the entries of\n\
        a .diacraft package copied as they were), carried (what the file carries: classes, document_level, macros, objects and signatures_stale, as the\n\
        engine's save result gives it: never a warning), checked (what the check after the save found, for a save through a model writer: the file is read\n\
        back and compared with the drawing, class by class, as {done, classes: [{class, words, count}], as_values, reason}, and null for a save into\n\
        the drawing's own file), warning_count and warnings (by class word, as roundtrip tells them)."),
    row("convert.drop-macros", 8, "--drop-macros", 24, "a drawing type: leave the macro project out of the file (without it, a type that cannot hold the project refuses the save)"),
    row("convert.drop-objects", 8, "--drop-objects", 24, "a drawing type: leave the embedded objects of the other format out of the file; each shape shows its picture"),
    para("convert.note1", 6, "A picture type is export's: the page is drawn as export draws it, and the line is export's with in. The options below are the picture's, and\n\
        for a drawing type each is a mistake in the line."),
    row("convert.out", 8, "--out PATH", 24, "where to write (replaced if it is there; never FILE itself, a link to it included). Written whole or not at all."),
    row("convert.page", 8, "--page N", 24, "a picture: the page's place in the drawing, from 1 (the default is the first page that is not a background)"),
    row("convert.dpi", 8, "--dpi N", 24, "a PNG: the pixels to the inch (96 by default, at most 2400)"),
    row("convert.text", 8, "--text MODE", 24, "an SVG or a PDF: text (the default) or outlines"),
    row("convert.transparent", 8, "--transparent", 24, "a picture: no white paper under the drawing"),
    row("convert.bundled-fonts", 8, "--bundled-fonts", 24, "a picture: the bundled faces alone (see Fonts below)"),
    blank(),
    synopsis("diacraft-cli roundtrip FILE --out PATH [--move DX,DY] [--bundled-fonts]"),
    synopsis("diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move|retext|delete|add | --move DX,DY] [--bundled-fonts]"),
    para("roundtrip.about", 6, "Opens the drawing, stencil or template FILE (a template as the file itself), optionally moves the first shape of the first page that has\n\
        one, saves it to PATH (the type is PATH's extension), and prints one JSON line of what the save changed: the SHA-256 of FILE, its variant,\n\
        the parts of the two packages (how many are the same bytes; which differ, are new or are gone; whether the content types and the\n\
        relationships say the same), whether the file read back equals the file read, and with --move the shapes and cells that differ. No\n\
        formula, no value, no text and no name a person made up. A file that does not open is a report with the class of the refusal, and the\n\
        exit status is 0. An OpenDocument drawing (.odg, .otg) is, until its reader exists, copied entry by entry to PATH (which keeps its\n\
        extension) and the line says whether every part is the same (parts_identical) and which elements differ, by path (element_diff).\n\
        A flat OpenDocument drawing (.fodg, one XML document, told by what the file is) is read, and written again unedited through the flat\n\
        writer, which is the file's own bytes: the line says form flat, what was read, saved and file_identical. PATH must be a .fodg (another\n\
        name is other_type and is not made), and --move is not supported."),
    row("roundtrip.out", 8, "--out PATH", 24, "where to write (never FILE itself)"),
    row("roundtrip.move", 8, "--move DX,DY", 24, "inches to the right and up, before the save"),
    row("roundtrip.bundled-fonts", 8, "--bundled-fonts", 24, "the bundled faces alone (see Fonts below)"),
    para("roundtrip.note1", 6, "With --via diacraft the file makes the trip through DiaCraft's own format: it is opened, edited if asked, saved as .diacraft (a stencil as\n\
        .diacraft-stencil, a template as .diacraft-template) in a scratch folder that is removed at the end, opened again in a new session, and saved to\n\
        PATH in the format FILE came in (PATH must end in a VSDX type for a VSDX package, or in .odg for an OpenDocument drawing). It prints one JSON\n\
        line: {via, format, input: {sha256, bytes}, opened, edit, diacraft: {bytes}, output: {sha256, bytes}, content: {compared, same, count,\n\
        differences}, model: {same}}. content is the content check of the design's section 4.3 on FILE and PATH, the first 200 differences by part,\n\
        path and kind, and the exact count; model says whether the file read back equals the drawing that was saved as .diacraft. A leg the engine\n\
        refuses is a report with refusal: {leg, error, words} and the exit status 0, content has compared false and same null, and nothing says same.\n\
        The file's content is never in the line: no formula, value, text or name a person made up."),
    row("roundtrip.via", 8, "--via diacraft", 23, "the trip through .diacraft (the only value)"),
    row("roundtrip.edit", 8, "--edit KIND", 24, "before the trip, a salted edit of the first shape of the first page that has one: cell (shape.format, line weight 1.23),\n\
        move (shape.move 0.125,0.125), retext (shape.set_text \"salt\"), delete (shape.delete), or add (shape.drop of the master\n\
        rectangle at the page's centre). --move DX,DY is the move edit with a delta of your own. Not both, and only with --via"),
    blank(),
    synopsis("diacraft-cli compare FIRST SECOND"),
    synopsis("diacraft-cli compare --content FIRST SECOND"),
    para("compare.about", 6, "Compares two OpenDocument drawings (.odg, .otg) and prints one JSON line: each file's SHA-256 and what its package holds, and what\n\
        differs between them, part by part (parts_identical) and element by element (element_diff), with the rules of roundtrip's line. A file\n\
        that does not open is a report with the class of the refusal, and the exit status is 0. A flat drawing (.fodg) is read, and has no\n\
        parts to compare: parts.comparable is false, with the class flat_not_compared.\n\
        With --content the two files are checked for the same content (the design's section 4.3), by the lists of their format: two VSDX packages,\n\
        or two OpenDocument drawings (a flat one is compared with a package once it is split the way the reader splits it). The line is {format,\n\
        first: {sha256, bytes}, second: {sha256, bytes}, content: {compared, same, count, differences}}. Two files of different formats are refused\n\
        in words (exit status 1), and so is a .diacraft file, which has no content check of its own. A file that does not open is a report with the\n\
        class of the refusal and the exit status 0."),
    blank(),
    synopsis("diacraft-cli completions SHELL [--program diacraft|diacraft-cli]"),
    para("completions.about", 6, "Prints the completion script of a shell on standard output, and nothing else: SHELL is bash, zsh, fish, powershell or elvish. The script completes\n\
        the commands, their options and the values an option takes, the command ids that run --cmd takes (the ones commands lists) and, where a file is read or\n\
        written, the folders and the files of the types that suit it. docs/cli.md says where each shell looks for the script."),
    row("completions.program", 8, "--program NAME", 24, "the program the script is for: diacraft-cli (the default), or diacraft, the app, which has options of its own"),
    blank(),
    synopsis("diacraft-cli man [--list | PAGE]"),
    para("man.about", 6, "Prints a manual page in the format the man program reads, on standard output: PAGE is diacraft-cli (the default), one of its commands (run, export, and\n\
        so on: the pages diacraft-cli-run and the rest), or diacraft, the app. Read one with `diacraft-cli man | man -l -`."),
    row("man.list", 8, "--list", 24, "print the names of the pages, the program's first, one on a line, and no page"),
    blank(),
    lead("fonts", "Fonts:", "`run`, `export`, `roundtrip` and `convert` to a picture, and `mcp` for a session of this process (not --connect, and in the default mode only once it has chosen\n\
        the headless session), read the machine's fonts once, at the start and before any command, and the commands that measure\n\
        text (a text block that grows with its text) and the pictures they draw use that one set, as the window does. --bundled-fonts reads nothing from\n\
        the machine: the bundled faces are the same everywhere, so the numbers and the pictures of a run can be compared between machines. `oracle` has\n\
        its own rule: it measures in the machine's faces, and leaves out the cells a stand-in face measured."),
    blank(),
    heading("options", "Options:"),
    row("options.version", 2, "-V, --version", 17, "Print the version and exit"),
    row("options.help", 2, "-h, --help", 17, "Print this help and exit"),
    row("options.color", 2, "--color WHEN", 17, "When to colour the help and the messages for a person: auto (the default), always or never. What a program reads is never coloured"),
    blank(),
    heading("environment", "Environment:"),
    variable("environment.rust_log", 2, "RUST_LOG", 17, "off, error, warn (the default), info, debug or trace: what is logged to standard error"),
    variable("environment.no_prefs", 2, "DIACRAFT_NO_PREFS", 17, "set to anything (empty too): `run` and `mcp` (a session of their own) read and write none of the person's settings (config.toml,\n\
        the dictionaries); what a run changes is kept in memory until it ends. A script or an agent that must not touch the settings sets it"),
    variable("environment.config_dir", 2, "DIACRAFT_CONFIG_DIR", 17, "the folder of the settings, in place of the platform's (~/.config/diacraft on Linux, ~/Library/Application\n\
        Support/diacraft on macOS, %APPDATA%\\diacraft on Windows); set but empty, as DIACRAFT_NO_PREFS. `run` and `mcp` (not --connect) share the\n\
        settings with the window, and say once on standard error what is wrong with them (a file that is broken, too big, not text, from a newer\n\
        build, or not writable)"),
    variable("environment.automation_read_root", 2, "DIACRAFT_AUTOMATION_READ_ROOT", 17, "the folders of --automation-read-root of mcp, a list written as PATH is (: between folders, ; on Windows), when the\n\
        flag is not given"),
    variable("environment.automation_write_root", 2, "DIACRAFT_AUTOMATION_WRITE_ROOT", 17, "the same for --automation-write-root"),
    variable("environment.no_color", 2, "NO_COLOR", 17, "set and not empty: no colour in the help and the messages, unless --color always says otherwise"),
    variable("environment.clicolor_force", 2, "CLICOLOR_FORCE", 17, "set, not empty and not 0: colour even where the output is not a terminal (NO_COLOR still wins)"),
    variable("environment.clicolor", 2, "CLICOLOR", 17, "0: no colour on a terminal"),
    variable("environment.term", 2, "TERM", 17, "dumb: no colour on a terminal"),
    blank(),
    lead("exit", "Exit status:", "0 on success, 1 when a step, a connection or a file fails, 2 for a mistake in the command line."),
];
