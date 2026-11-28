# How to write help

This folder is DiaCraft's help: the topics the Help pane shows (they are part of the program, so help works with no network
connection), the tables that connect the window to them, and, for the topics that are published, the pages of the website. One source
makes both. `cargo xtask help check` holds every file here to the rules below, and the program reads the files with the very same code,
so a topic that passes the check is a topic the pane can draw.

## What is here

| Path | What it holds |
|---|---|
| `topics/<folder>/<id>.md` | One topic per file. The file is named by the topic's id. The folders only keep things tidy: a topic keeps its id when its file moves. |
| `sections.tsv` | The sections of the help home, and the topics each lists. |
| `tips.tsv` | A description and search keywords for each control and command. |
| `map.tsv` | The topic of each place in the window (a control, a dialog, a pane), so that help can open the right topic from it. |
| `published.txt` | Every topic id that was ever published on the website. An id on this list may never disappear. |

The three tables are tab-separated, one row a line. A line that starts with `#` and a blank line are skipped. In a column, `\n` is a new
line, `\t` a tab and `\\` a backslash; no other escape exists. A row that breaks a rule is refused by the check and skipped (and logged)
by the program, so a mistake in one row never hides the others.

## A topic

A topic starts with a header: a line of three dashes, one `key: value` per line, a line of three dashes. Then the body.

| Key | Meaning |
|---|---|
| `id` | Lowercase words joined by dashes (`page-setup`), at most 64 characters. Unique, and never changed once published. |
| `title` | At most 80 characters. |
| `summary` | At most 160 characters. It is the snippet that search shows. |
| `keywords` | Words for search, separated by commas, at most 32. Write the words people would type, not only the words the topic uses. |
| `unlisted` | `yes` for a topic the help home does not list. |
| `site` | `yes` (the default) when the topic is a page of the website, `no` when it is only in the program. |
| `description` | For the website: 140 to 160 characters. Required when `site` is `yes`. |
| `seo_title` | For the website: at most 60 characters, when the page's title should differ from `title`. |
| `picture` | For the website: the name of the page's social picture. |
| `path` | For the website: where the page is (`help/<id>` when absent). |

An unknown key is an error for the check and is ignored by the program.

### The body

A body is blocks. A blank line ends a block, and so does a line of another kind.

| Block | Written | Notes |
|---|---|---|
| Heading | `## Text` or `### Text`, optionally ending with `{#anchor}` | There is no `#`: the title comes from the header. The anchor is the text of the heading as words joined by dashes, unless given. Anchors are unique in a topic. |
| Paragraph | lines of text | The lines are joined with one space. |
| List | `- item` or `1. item` | One nested level, by two spaces. An item continues on lines indented by two spaces (four for a nested item). A list does not mix numbered and bulleted items. |
| Note | lines that start with `> ` | One box of one paragraph. A note holds no note. |
| Code | a line of three backticks, an optional word (`text`, `sh` or `json`), the lines, a line of three backticks | Shown as it is, and never translated. |
| Table | a row of cells between pipes, a separator row such as `|---|---|`, more rows | At most 6 columns and 60 rows. Cells hold the inlines below. Write `\|` for a pipe inside a cell. |
| Picture | `![alt text](site:name)` alone on its line | `name` is a picture of the website. The program shows the alt text as a caption, so write alt text that says what the picture shows. |

There is no HTML, no heading underlined with `=` or `-`, no indented code, no reference link, no thematic break and no footnote. The
check refuses each of them and names the line.

### Inlines

`**strong**`, `*emphasis*` (nested once at most), `` `code` `` and links `[text](target)`. A star, a bracket, a backtick or a backslash
that is only itself is written with a backslash: `\*`, `\[`, `\]`, `\(`, `\)`, `` \` ``, `\|` and `\\`. Delimiters pair from left to right;
one left open is an error. An underscore is only an underscore.

### Links

| Target | Means |
|---|---|
| `topic:id`, `topic:id#anchor`, `#anchor` | A topic, a place in it, or a place in this topic. |
| `ctl:ControlId` | A control of the ribbon. The program shows the control's name and key as they are in the window, and a click shows where the control is. It never runs it. |
| `cmd:command.id` | A command that no control reaches, shown the same way. |
| `key:Cmd+Shift+Z` | A key. `Cmd` is Command on a Mac and Ctrl elsewhere; the modifiers are `Cmd`, `Ctrl`, `Alt` and `Shift`. The program writes the key the way the platform does, and the link text is not used. |
| `https://diacraft.dev/…`, `https://diacraft.app/…`, `https://github.com/intergrado/diacraft/…` | The website, the web app, the public repository. |

Any other address is refused. A link to a topic or an anchor that does not exist is refused by the check.

## Writing rules

- Short plain sentences. Steps are a numbered list in the imperative: "Choose Page Setup."
- Write the window's names as the window shows them, as `ctl:` and `cmd:` links, so that they follow the labels and keys when those change.
  Write keys as `key:` links.
- A topic is about one task. It is 150 to 800 words. Put what a person looks for first.
- Say what DiaCraft does not do yet where a person would look for it, in the words of the control's tip: "not available yet".
- Write from the program as it is: run it and try each step. Claim nothing about any other program, and name none. The one exception is a
  file-type label or a list of the formats DiaCraft opens and saves, which is written exactly as the window writes it.
- A topic needs no screenshot to be followed.
- When a task adds or changes a command, a control, a dialog or a pane, it changes the topic, the tips row and the map row of that place in
  the same change. The tests that cover the window fail otherwise.

## Checks

`cargo xtask help check` (a step of `cargo xtask ci`) holds that: every topic parses within its limits; each header is complete, with the
site fields when the topic is published; ids are unique and none that was published is lost; every `topic:` link and `#anchor` resolves;
every `ctl:` names a ribbon control and every `cmd:` a registered command; every `key:` is a key; web links go only to the three sites;
every topic is in `sections.tsv` or is `unlisted`; the three tables are well formed and name only topics, controls, commands and places
that exist; and the words follow the project's rules about names (`cargo xtask brands`).
