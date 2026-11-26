# Feature comparison matrix

The data of the feature comparison page of the website is [`parity-matrix.toml`](parity-matrix.toml), and `cargo xtask site --write`
makes the page (`site/feature-comparison.html`) from it; [the website's notes](website.md#the-feature-comparison-page) say how. It compares DiaCraft
with the reference app, LibreOffice Draw and draw.io in 155 rows grouped by area, and lists the features that only DiaCraft
has. Each cell is Full, Partial, Not yet or unknown (the page shows unknown as "?").

**The cells come from public documentation, not from running the apps.** The reference app was not inspected, nothing was
launched or measured, and our output has not been opened in it by the project. LibreOffice was also run as a black box, to
read the list of its file filters, its Gallery themes and the handles of its preset shapes. Every LibreOffice and draw.io cell
names its sources and the date they were read; the reference app's cells rest on the checklist. **Corrections are welcome:**
open an issue at <https://github.com/intergrado/diacraft/issues> with the row id, the cell and a public source.

## The file

TOML, because each row holds lists (its checklist rows, each cell's sources) that a flat table would have to pack into one
field. `version`, `values`, `[rules]` and `[sources]` come first, then one `[[area]]` per area in page order, one `[[row]]`
per row, and one `[[only]]` per candidate DiaCraft-only feature.

| Field of a row | What it holds |
|---|---|
| `area`, `id`, `feature` | The area's key, a stable id (lower case, hyphens) and the row's name as the page shows it. Names are neutral; the page may name the reference product. |
| `checklist` | The [checklist](parity-checklist.md) rows the row covers, by their Feature name (a Markdown link in a name counts as its text). Every one of the 977 rows is in exactly one row here. |
| `diacraft` | `{ rule = "checklist" }`, or `{ rule = "manual", value, source, note }` for the rows no checklist row covers. |
| `reference` | `value`, `edition`, `source` (a key of `[sources]`) and `note`. |
| `[row.libreoffice]`, `[row.drawio]` | `value`, `basis`, `sources` (addresses), `read` (the date) and `note`, which says what the sources show and what they do not. |

Two checklist names hold a vendor's name that `cargo xtask brands` denies; their lines end in a `# brand-ok` comment, the
mark the check accepts for a name that has to be kept.

## Rows by area

| Area | Rows |
|---|---|
| Drawing and editing | 20 |
| Shapes and stencils | 11 |
| Connectors | 11 |
| Text | 11 |
| ShapeSheet and formulas | 6 |
| Layers, containers and themes (fill, line and effects included) | 13 |
| Pages | 7 |
| Diagram types | 29 |
| Data | 7 |
| Files and formats | 10 |
| Export and print | 9 |
| Collaboration and review | 9 |
| Automation and developer | 5 |
| Platforms | 7 |
| **Total** | **155** |

## How each column is filled

**DiaCraft**, recomputed at every release from the checklist: a row is **Full** only if every checklist row it names is D,
**Not yet** if every one is M, and **Partial** otherwise. The seven rows with no checklist row (the VSDX, OpenDocument and
draw.io formats, and the four platforms) take a `manual` value from the doc their `source` names
([file format parity](file-format-parity.md), [hardware parity](hardware-parity.md)), read again for each release.

**The reference app**: a row of checklist rows is Full, because the checklist rows are the desktop edition's features, each
taken from the vendor's public support pages. `edition` says where the vendor's comparison of its plans puts the feature:
`desktop` (no plan mark found), `plan2` (the subscription with the desktop app, not the two web plans), `plan1` (Plan 1 and
Plan 2, not the web app of the business suites), `mixed` (parts in different plans; the note says which), `web` (the web app
or an online service) and `none`. The rows that are not Full are the OpenDocument and draw.io formats (not among the formats
its pages list), macOS and Linux (the desktop app is for Windows) and the browser (Partial: the web app has fewer features).
The vendor's pages were read on 2026-10-31, and its MCP server's documentation on 2026-11-26.

**LibreOffice Draw**: the help at `help.libreoffice.org/latest` (read 2026-11-26), The Document Foundation's own feature
comparison on its wiki (revision 886199, read on 2026-11-26 from the Internet Archive's copy of 2026-07-14, as the wiki now
answers automated readers with a challenge page), and LibreOffice 26.8.2.2 run as a black box (2026-11-26).

**draw.io**: the documentation at `www.drawio.com/docs` (read 2026-11-26; a few pages were downloaded on 2026-11-25) and,
where the documentation is silent, the Apache-2.0 editor source `jgraph/drawio` at commit `44dc33f` (its menus and its table
of interface strings).

| `basis` | Meaning |
|---|---|
| `documented` | A public page says so. |
| `oracle` | Seen by running LibreOffice as a black box (its filter list, Gallery themes or shape handles read over UNO). |
| `absent` | Not found in the product's documentation read (for draw.io, nor in its interface strings). A Not yet on this basis says the feature was not found, not that it was proven missing. |
| `unknown` | No source settles it; the note says what would. |

**Partial** means the sources show part of what the row's checklist rows describe; the note names the parts shown and the
parts not found. Full needs every part.

Counts at this commit (`cargo xtask site` prints DiaCraft's, which move with the checklist; the other columns move only when their cells are read again):

| Column | Full | Partial | Not yet | unknown |
|---|---|---|---|---|
| Reference app | 150 | 1 | 4 | 0 |
| LibreOffice Draw | 22 | 79 | 49 | 5 |
| draw.io | 17 | 116 | 22 | 0 |
| DiaCraft | 14 | 79 | 62 | 0 |

## Features only DiaCraft has

Each `[[only]]` item cites DiaCraft's docs and says, for each of the other three, whether it `lacks` the feature (`yes`,
`no` or `unknown`) and on what basis. **The page lists an item only when every column it publishes says `yes`.** All five
items ship. Three qualify: opening and saving both VSDX and OpenDocument drawings with Save As between them; the `.diacraft`
format, which has a public specification and keeps the details of a VSDX file or an OpenDocument drawing it was made from; and a
browser version that works with no connection once it has loaded (it asks for two files of its own the first time they are
shown), PDF and picture export included. Two do
not: the MCP server and command line (the reference app's vendor and draw.io both publish MCP servers) and the control
channel (each of the others can be driven by another program). Doubled connection points are not listed: they do not ship.

## Names and notices on the page

- draw.io is written in lower case, as plain text, with no logo. The page says "draw.io is a registered trademark of draw.io
  Ltd." and that DiaCraft is not affiliated with or endorsed by any of the companies named.
- LibreOffice, per The Document Foundation's trademark policy (revision 266041, read on 2026-11-26 from the Internet
  Archive's copy of 2026-11-01): a factual reference to its products needs no permission; the name is used in its exact form
  (never shortened), its first or most prominent mention carries "®" and is set apart from the text around it (a column
  header in bold does this), the page claims no link with the foundation, and no logo is used. The footer already says that
  LibreOffice is a registered trademark of The Document Foundation and that DiaCraft is not affiliated with or endorsed by it.
- The reference app's own trademark line is the footer's.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-26 | minor | `cargo xtask site --write` makes the page from the data; DiaCraft's counts follow the checklist (79 Partial and 62 Not yet, one row having moved since the research); the wording of two features only DiaCraft has says what the docs say (the format's public specification and kept details; no connection once loaded, with the two files the editor asks for), and a note of the offline item names the vendor's plan comparison |
| 2026-11-26 | major | Created, with the data file of 155 rows, the rules of each column, the sources and their dates, the DiaCraft-only items and the notices the page carries |
