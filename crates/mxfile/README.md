# diacraft-mxfile

The reader of draw.io files for DiaCraft, named for the `mxfile` root element of those files (the design of the import and its plan are the project's; the contract is the draw.io specifications under `/`).
It is a layer-3 crate, a file format beside `odg`, `vsdx` and `native`: it finds a drawing in a file, decodes its pages and reads them into the ShapeSheet model within budgets.
It is import only, and **so far its pages, cells, style strings and labels are built**: the sniff, the compressed page,
the walk of a file to its pages, for each page its settings, its `defs` and its cells, repaired into a tree with their places on the page, the reader of a cell's style string, and the reader
of labels, which no stage calls yet. The pages of the drawing come out empty and named; the shapes and connectors the model is made of are the tasks that follow (and the
rest of the plan), and the engine does not open these files yet.

## What it reads

`read(&[u8], &Limits) -> Result<Read, DrawioError>` takes the bytes of a file and gives a `Read { doc, report }`: a new `Document` with a page for each page of the file, and a
`Report` with the form of the file, the warnings by class (a count and up to 200 named cases each), the connectors to route and the work the read did.
`file::read_sources` is the same walk that hands each page on with what it holds (`PageSource`: its place, id and name, its settings, its `defs` and its `Graph` of cells) and gives the file's own properties.

* **By its bytes, never by its name** (`sniff`). A PNG signature is an image that may hold a drawing; otherwise the bytes are text
  (UTF-8 with or without a byte order mark, UTF-16, US-ASCII) and the name of the root decides: `mxfile` is a file of pages, `mxGraphModel` and `diagram` are one page with no
  wrapper, `svg` is an image that may hold a drawing, `mxlibrary` is a library of shapes. The sniff looks at no more than 64 KiB (a mark, a declaration, comments, a DOCTYPE, white
  space) before the root's name, and claims nothing else, so a VSDX, an `.odg` or a `.diacraft` file (ZIP archives) is never read as a drawing.
* **The pages**. An `mxfile` is walked **one `diagram` at a time** through the container's stream of the root's children, so that what is held is one page's tree and
  not the file's. A page is named by its `name`, or `Page-N` for its place; a name is cut to 255 characters, has its control characters made spaces and is made unique
  (`A`, `A (2)`, `A (3)`). A page with no id takes its place (from 0) as its id, and one that repeats an earlier id is counted. Children of `mxfile` that are not pages are counted and
  skipped; a file with no page is one empty page.
* **The compressed page** (in `decode`). The page's text is Base64 of raw DEFLATE of the URI-encoded XML, and the reader reverses the steps: the forgiving Base64 of
  browsers (white space anywhere, missing padding), the container's bounded inflate, `decodeURIComponent`, the clean of the characters XML cannot hold, and the parse. XML that was
  never URI-encoded and a zlib stream are read and say so. A page that fails a step is **empty, with a warning that names the page and the step**
  (`drawio_page_undecodable`), and the other pages are read. Each step is public on its own, so that the vectors of the specification hold each to its text.
* **A file that is one page** (a bare `mxGraphModel`, or a `diagram`) is a page named `Page-1` or by its name.
* **The file's own properties** (`file`). The `vars` of the `mxfile` are read from their JSON by a reader of the crate's own (one pass, an explicit stack for nesting, 64 levels at the
  most): text, numbers as written and flags, a name written twice with its last value at its first place, at most 320,000 names kept **as they are read**, so that a member past the cap is
  checked and skipped and never held. No other attribute of the `mxfile` changes the drawing.
* **A page's model** (`file`). Its settings, held to the ranges that keep a page's size a number: `pageWidth` and `pageHeight` (850 by 1,100 when missing or no number, held to 1 to
  1,000,000 px), `pageScale` (1, held to 0.001 to 1,000), `background`, `math`, `shadow`, and the `src` of `backgroundImage` (the last member of that name, read from the JSON as `vars`
  is, and nothing else of it kept). The `data` of each `def` of the `defs`, by position, never expanded here. The editor's own settings are skipped.
* **A page's cells** (`cells` and `geom`). The flat list of the `root` becomes a `Graph`:
  * **Wrappers**: any element that is not an `mxCell` is the cell's user object (`label`, `tooltip`, `link`, `linkTarget`, `tags`, `placeholders`, and every other attribute as a
    property in the order written), its first `mxCell` carrying the rest; the wrapper's id and label win over the cell's.
  * **Ids**: the first cell with an id owns it, and every later cell with it is kept under an id of the reader's own (`<id>~2`, `<id>~3`, probing past the ids the file has), while every
    `parent`, `source` and `target` that names the id means the first.
  * **Parents** (and the design's amendment of R24): **one linear walk**, each cell visited once by an explicit stack. The root is the cell `0`, else the first that is neither vertex nor
    edge, else the first. A cell with no parent, a parent that is not in the page, a parent that is itself or a cycle of parents, or a chain deeper than 200 levels is moved to the end of the first
    layer in the order of the file, and counted (`drawio_parent_missing`, `drawio_parent_cycle`, `drawio_too_deep_tree`); with no layer it is left out (`drawio_cell_dropped`).
  * **Roles**: the root, a layer (a child of the root, whatever its flags say), a shape, a connector (a cell with both flags too), a connector's label (a relative shape in a connector), a port (a
    relative shape in a shape), and a cell that is neither vertex nor edge, whose children are walked as a group's.
  * **Places**: a cell's box on the page is the sum of its own and its ancestors' (an absolute geometry from its parent's origin, a port from the share of its parent's size plus its offset),
    held within 10,000,000 px at each step. A connector has no box (its waypoints are measured from its parent's origin), and nor has a label (its place needs the connector's path).
  * **Numbers**: every number is a finite decimal number within the cap, or 0; a size below 0 is 0; at most 10,000 waypoints a connector. Each repair is counted, once a page, with how many.
  * **Skipped, and counted**: an attribute that the table does not list (once a page, with the first names), and an element that is not read (by its name).
* **Labels** (`html`, S-R24 to S-R27). `read_html` reads a label whose cell has `html=1`, and `read_plain` one that has not, into a `Label`: its
  paragraphs, each with the alignment its block says and its runs (text, and a `RunFmt` that says only what the label's markup says: bold, italic, underline, strike, sub or
  super, a family, a size in px, a colour), and the first link it holds when it is `http:`, `https:`, `mailto:` or a page link. **DiaCraft never renders HTML**: the tokenizer is the
  crate's own (adapted from WordCraft's, `docs/provenance.md`), and what the table of S-R25 does not name keeps only its text. `b` `i` `u` `s` `sub` `sup` `font` `span` `a` give
  a format; `div` `p` `h1` to `h6` `li` `tr` `blockquote` `pre` end the paragraph; `br` is a line break (U+2028) in it; a list item starts with "• " or "1. "; a table row is a
  paragraph with a tab between its cells; `style` is read for `font-family`, `font-size`, `color`, `font-weight`, `font-style`, `text-decoration` and `text-align`. `script`,
  `style` and `template` go with what is inside; `img`, `svg`, `math`, `iframe`, `object`, `form` and the rest that cannot be shown go with it too and are counted
  (`drawio_label_content_dropped`); any other element is its text. The six named references of S-R25 and numeric references are characters, any other name is left as it is, and a
  character XML cannot hold is taken out. White space collapses as HTML collapses it, and a `br` at the end of a block makes no line. In a plain label a line feed is a line break and
  nothing else is read; with the drawing's `math` on, a label with text between `$$` and `$$` is shown as it is and counted (`drawio_math_shown_as_text`).

## Style strings (`style`, S-R1 to S-R9 and S-R17 to S-R23)

A cell's look is one `style` attribute. `StyleCache::resolve(text, kind, &mut Reading)` gives a typed `Style`, and each distinct text of each kind is parsed once and shared.

* **One pass, no trimming** (S-R1). The string is split at every `;`; a piece with `=` is a key and a value (the value is everything after the first `=`), a piece without one names a style of
  the stylesheet (a name it does not have is ignored), an empty piece is nothing. Case counts. A text value is a place in the style's own text, never a copy.
* **Defaults and named styles** (S-R2, S-R7, S-R8). A shape and a connector start from the stylesheet's defaults, unless the string starts with `;`; the pieces then apply left to right, a named
  style's keys copied in where it stands, and a value of `none` takes its key away (S-R4). The list of named styles, with the nine colours and their plain twins, is `named.rs`.
* **Typed by the key** (S-R5, S-R9, S-R17). A number is a finite decimal within the read's coordinate range (`0x2`, `Infinity`, `1e999` are not), a flag is on for a number but 0 and for `true`,
  a colour is `#rgb` to `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, a CSS name, `light-dark()` (its first colour) or one of six keywords, and a word is one of its key's. A value that is not
  what its key reads is dropped, so that the built-in value of S-R9 applies (a colour: `default`), and **one `drawio_style_bad_value` names the key**; an empty value is no value; nothing fails a cell.
  A colour of more than 256 bytes is no colour before it is looked at.
* **Colours against the parent** (S-R17). `Style::paints(parent)` resolves `default` (the light-mode colour of its key), `inherit`, `parentFillColor` and `parentStrokeColor` against the parent's
  colours (a layer's children get `default`), and `fillColor` and `strokeColor` against the cell's own: one level, never a loop.
* **Keys it does not read** (S-R6, S-R38). They change nothing, and `StyleCache::finish` puts the twenty most frequent in `drawio_style_keys_ignored`, counted once for each cell that has them.
* **Budgets.** The bytes of a text are charged when the cache hashes it and again when the grammar walks it; each piece and each key a named style adds are charged before they are applied
  (`ReadWork::style_bytes`, `style_pieces`). A megabyte of `;`, two hundred thousand named styles, a million keys and a colour of a million `light-dark(` each cost what they are.

## What it refuses

A refusal is a `DrawioError` with a kind, a class word of the specification and words that name no content beyond a page's name, cut and escaped:

| Class | When |
|---|---|
| `drawio_not_a_drawing` (`NotMine`) | any other root, a ZIP, an image with no drawing (SVG and PNG are claimed by the sniff and read), anything else: the engine goes on to the next reader |
| `drawio_library_not_drawing` | the root is `mxlibrary` |
| `drawio_encoding_unsupported` | a declared encoding other than UTF-8, UTF-16 and US-ASCII |
| `drawio_entity_refused` | a document type or an entity declaration, in the file or in a decoded page |
| `drawio_too_large`, `drawio_ratio_exceeded`, `drawio_too_deep`, `drawio_too_complex` | a cap below |
| `xml_malformed` | XML that does not parse |

## The caps (`limits.rs`, every one a field of `Limits` too)

| What | Cap | Past it |
|---|---|---|
| The file | 256 MiB | `drawio_too_large`, before a byte is looked at |
| One page's XML, plain or inflated and decoded | 64 MiB | `drawio_too_large`, the byte past it the last one made |
| The XML of all the pages, decoded | 256 MiB | `drawio_too_large`: the sum is capped, and not only each page |
| An inflated page against its compressed size, past 1 MiB | 200 to 1 | `drawio_ratio_exceeded`, at the first chunk that shows it |
| XML depth, the root's level included | 64 | `drawio_too_deep` |
| Items of one page's XML tree | 6,000,000 | `drawio_too_complex` |
| Elements of one page's model | 2,000,000 | `drawio_too_complex`, counted before any cell is read |
| Cells of one page (every element of the `root`) | 1,000,000 | `drawio_too_complex`, counted before any cell is read |
| Levels of cells below a layer | 200 | the cell is moved to the first layer, `drawio_too_deep_tree` |
| One attribute value | 32 MiB | `drawio_too_large` |
| Pages | the model's 1,000 | `drawio_too_complex` |
| Waypoints of a connector | 10,000 | the rest dropped, `drawio_too_many_points` |
| A coordinate, a size | 10,000,000 px | held there, `drawio_bad_number` |
| A page size | 1 to 1,000,000 px | held to it, `drawio_bad_number` |
| A number's text | 64 bytes | no number, `drawio_bad_number` |
| `vars` kept, levels of JSON | 320,000, 64 | the rest checked and counted (`drawio_too_many_properties`); deeper is not read (`drawio_attribute_unreadable`) |
| A label, HTML or plain | 1 MiB | cut at a character, with `drawio_label_too_long` |
| Elements of a label that are followed | 64 levels | the markup below is read as the text of the 64th |
| The read's work | 1,000,000,000 steps, to start | `drawio_too_complex` |

Every loop charges the units it walks (a byte, a character, an event, a page, a node of the tree, a cell, a byte of JSON, a step of a label) to the read's work **before it runs** (`Reading::charge`), the inflate being
charged for what it made since its own cap bounds it, and a charge that would pass the budget is refused and not counted. The cells are charged five passes each (the ids, the cycles and the root, the
levels, and the lists and places twice). What a read cost is in `Report::work`, and `read_counted` gives it on a refusal too. Measures these numbers on crafted files and may lower them.

## What it never does

It never writes a draw.io file. It never renders HTML, and never hands a label to a browser engine, a web view or a script. It never fetches anything a file names (a picture's address, a font, a link, a library) and never runs anything in it (actions, animations,
scripts); the XML is the container's, which refuses a DTD and expands no entity. No input makes it recurse, panic or allocate in proportion to a claim the file makes: the
output of the inflate is held to its cap and ratio before it is kept, the Base64 and URI steps make output no larger than their input, a page's tree is consumed as its cells are read
(strings are moved, not copied) and dropped before the next page is read, the elements and the cells of a page are counted and held to their caps before any is read, and a JSON attribute is
read with its cap applied as its members go by. It uses no clock, no file system and no network, and it builds for `wasm32`.

## Tests

`tests/it` holds the vectors of the specification (`file_vectors.rs`: file rule by rule; `style_vectors.rs`: the style spec's and the rules of S-R5 to S-R9 and S-R17; each test is named after its vector or its rule), the sniff
(`sniff.rs`), the small functions of the steps on every input they can have (`steps.rs`), and the hostile inputs, counted in the units each loop walks (`hostile.rs`): a 100 MiB
bomb, a page of 64 MiB and one byte more, Base64 of 64 MiB of white space, 64 MiB of `%41`, a page of nothing but U+FFFE, nesting a million deep, a file whose pages are each under the
floor of the ratio and together past the cap, and, for the cells, **a chain of a million cells each the parent of the next, a cycle of a million, a million cells of one id, a million ids made to
collide with the renamed ones, a page of 2,000,001 elements, a million cells exactly and one more, a hundred connectors of 10,001 waypoints, 300,000 cells that each have three things wrong, JSON
nested a million deep and of a million members, and a child process under `ulimit -v` that reads the `vars` of five million members and a `backgroundImage` of four million**, each with its work
counted (five steps of cells for each cell, and the bytes of JSON once). The cases have `#[ignore]`d twins that time the same inputs beside a yardstick
(`cargo test -p diacraft-mxfile -- --ignored`). The inputs are built in code by `diacraft_testkit::mxfile`; no file made by another program is read and none is committed. `fuzz/` is the
cargo-fuzz crate of the targets `mxfile_decode`, `mxfile_read`, `mxfile_style` and `mxfile_label`, whose laws (`fuzz/src/lib.rs`, `fuzz/src/read.rs`, `fuzz/src/style.rs` and `fuzz/src/label.rs`) the tests include and hold a seed corpus and random bytes to. `style_hostile.rs` counts the hostile styles (32 MiB of `;`, 200,000 named styles, a million keys, a million cells of
one style, numbers that are NaN, infinite or huge, degenerate colours) and has timed twins beside a yardstick; `style_props.rs` holds the property that a style resolved in one pass equals its pieces applied by hand.

The labels have three files and a target of their own. `label_table.rs` holds the reader to each row of S-R25's table, to S-R26 and S-R27 and to the work it counts;
`style_vectors.rs` holds style in its `label` module, beside the `style_strings`, each test named after its vector; `label_hostile.rs` has the hostile labels, counted in steps (a label of 1 MiB and one byte more, `<b>`
349,000 deep, 100,000 unclosed `<script`, a million `&amp;`, a mebibyte of stray `&`, numbers past the last character, an attribute of 900,000 bytes, 262,144 `<br>`, 200,000 list
items with no end tag, nested `<svg>` ...), with `#[ignore]`d twins beside a yardstick of labels as the editor writes them. The fuzz target `mxfile_label` (`fuzz/fuzz_targets`) reads
bytes as a plain label, an HTML label or one made of pieces a recipe chooses, with the caps made small now and then, and holds the reader to its law (`fuzz/src/label.rs`, which the
tests include as `label_fuzz.rs`).
