# diacraft-help

Offline help for an app, all of it bounded and none of it networked. The crate holds what an app needs to show its own help and what its tests and its checker need to
hold that help to the rules: a small Markdown format with one parser, translation of the help into other languages, ranking of commands and topics against a query,
the tables that connect an app's controls to its topics, and the web addresses a help link may open. It names no app: what belongs to the app that uses it is one file,
[`src/host.rs`](src/host.rs). It is built inside DiaCraft and shaped to move to the shared craft libraries as `craft-help` with no change to its logic.

It depends on `unicode-normalization` and nothing else, has no TOML, serde, logging or network code, `forbid`s `unsafe`, and builds for the browser (`wasm32`).

## What is in it

| Module | What it holds |
|---|---|
| `md` | **Help Markdown.** `parse` reads a topic: a header, then headings, paragraphs, lists of two levels, a note, a code block, a table and a picture, with the inlines emphasis, code and links. Every rule it holds a topic to is a `Code` of a `Problem` with its line, and a topic that breaks one still reads (what breaks a rule is read as plainly as it can be). The parser that checks a topic in a build is the one that renders it in the app. |
| `header` | The header of a topic: `id`, `title`, `summary`, `keywords`, `unlisted`, and the fields only a website reads (`site`, `path`, `description`, `seo_title`, `picture`). |
| `catalog` | **Help catalogs**, one language's translations of a topic's segments, and `translate`, which puts them in. A segment with no row stays in the source language; a translation that breaks Help Markdown, or changes where a link goes, a key or a code span, is skipped, so a translation can never make a link go elsewhere. `pack` and `unpack` make the one file a browser fetches for a language. |
| `fold` | Folding for matching: the language's own lower case (with the Turkish pairs of i), decomposition with the combining marks dropped, white space collapsed, and `cut_query`. |
| `rank` | `rank` orders commands against a query: every word of the query must start a word of the label, the place or a keyword, in the display language or in English; a label that starts with the query comes first, then the most recent, then the host's order. There is no fuzzy matching. |
| `index` | `TopicIndex`: a search over topics by title, headings, keywords, summary and body, in that order of weight. |
| `tables` | The three small tables of a help tree: tips (a description and keywords for each control and command), the map (the topic of each place of the window) and the sections of the help home. |
| `url` | `allowed`, which of the host's sites a link may open (read the way a browser reads it: no dot segment in the path in any spelling, no port, no user name, no broken escape), and `feedback_url`, the address of a new-issue page with a body filled in. |
| `chord` | The keys a help text names (`Cmd+Shift+Z`), read, and shown the way each platform writes them. |
| `nav` | `History`, the places a person has been to, and `next_row`, the movement between the rows of a list. |
| `host` | The host's values: the sites a link may open, the repository of the feedback page, where the help tree is. |
| `Bundle` | A help tree read for use. `embedded()` is the tree this crate was built with (`build.rs` embeds the host's help tree on a desktop target and only the tables on `wasm32`), `from_parts` builds one from text the caller has. |

## Limits

Every size has a named constant in `src/lib.rs`, the same for every path a text arrives by (an embedded tree, a fetched file, a contributor's folder). A text past one is
refused whole or cut as the table says, with a problem that names the line. Parsing is linear in the bytes of a topic (one pass over the lines, one over the characters of
each block, no backtracking); a ranking is linear in the candidates times the words of the query.

| Limit | Value | Past it |
|---|---|---|
| A topic, a language's topics together, topics in a bundle | 256 KiB, 4 MiB, 1,000 | refused, refused, the rest ignored |
| A line, a header | 16 KiB, 8 KiB | the topic is refused |
| Blocks in a topic, spans in a run of text | 4,000, 512 | the rest is dropped |
| Levels of a list, a table | 2, 6 columns by 60 rows | flattened, the extra cut |
| A link's target | 512 bytes | the link is shown as its text |
| A query | 255 characters, 16 words | cut |
| Candidates scored, rows of the search list, topics returned | 2,000, 5 commands and 5 drawings and 1 help row, 50 | the rest are not looked at |
| Words in an index | 400,000 | the rest are not indexed |
| Places in a history | 64 | the oldest is dropped |
| Rows of the tips table and of the map, a description, keywords | 5,000 each, 400 characters, 32 | the row or the rest is skipped |
| A catalog, its rows, a row | 2 MiB, 20,000, 16 KiB | refused, the rest skipped, the row skipped |
| The body of a feedback address | 1,500 bytes, percent-encoded | cut between characters |

## What it never does

It never panics (a lint denies `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` and `unreachable!` in the crate, and `unsafe` is forbidden), never opens a file or a
connection at run time (the build script reads the help tree once, at build time), never follows a link, and never runs a command: a link names a place, and what the app does
with it is the app's. A web link is built only for an address on one of the host's sites.

## Using it

```text
let parsed = diacraft_help::parse(text)?;          // a topic: header, blocks, and what broke a rule
let bundle = diacraft_help::embedded();            // the help tree this crate was built with
let topic = bundle.translated("page-setup", "de"); // English, with a language's rows where they are right
```

An app changes `src/host.rs` (its sites, its repository, where its help tree is) and writes its help as the README of its help tree says. A checker (in DiaCraft, `cargo xtask
help check`) reads the same files with the same parser and fails on any problem.

## Proofs and tests

Every cap has a counted test with an ignored timed twin (`cargo test -p diacraft-help -- --ignored`), and four fuzz targets live in `fuzz/` (`help_markdown`, `help_catalog`,
`help_query`, `help_tables`): `cargo fuzz run <target>` from that folder, on a nightly toolchain.
