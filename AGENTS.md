# AGENTS.md: rules for every contributor and coding agent

DiaCraft is an open-source, pure-Rust diagramming application. It runs natively on macOS, Windows, Linux and FreeBSD, and on the web
through WASM. It follows the conventions of the storytold craft apps (VectorCraft, DeckCraft and their siblings) but is an
independent project, not an ArtCraft product.

These rules come before any task, instruction or deadline. [`CONTRIBUTING.md`](CONTRIBUTING.md) says how to build, test and send a change.

## Start here

1. Read `ROADMAP.md`, then `docs/gaps.md` (the ranked work list), `docs/roadmap.md` and `docs/architecture.md`.
2. `docs/development.md` lists the tools and the gates, and `docs/editing-commands.md` what each command does.

## Where behaviour and code may come from

1. Behaviour comes from public information only. Never read or copy anything inside another vendor's install: binaries, stencils,
   templates, help files, fonts. Files people send us are not a source.
2. Never copy another vendor's documentation beyond feature, menu, cell and function names and other names needed for
   interoperability.
3. Never copy GPL, AGPL, LGPL or MPL code, and never depend on GPL, AGPL or LGPL code. Unmodified MPL-2.0 dependencies are allowed with
   a `NOTICE` entry; `cargo xtask deny` enforces the licences, and the entry is by hand. `cargo xtask licenses` writes the notices that the
   dependencies' licences ask to travel with the programs (`THIRD-PARTY-LICENSES.txt`, in every download), and its `--check` step of
   `ci` fails on a crate with no licence text or a licence file outside `deny.toml`'s list: after a change to `Cargo.lock` run it and
   commit the file. Nobody reads copyleft source.
   **One named exception: a CI-only deploy tool, never shipped.** The release workflow's `deploy` job installs `wrangler` from
   `packaging/cloudflare/package-lock.json` to publish the websites, and a dependency of it, `sharp`, brings the library libvips under the
   LGPL. The tool runs only on the release build machine and is never linked into or shipped with DiaCraft, and nobody reads its source. The
   exception covers that one tool and no crate, program or download: `cargo xtask deny` still refuses an LGPL crate.
4. Permissive sibling craft code may be copied, with a row in `docs/provenance.md` (repository, commit, path and what changed). Copy it
   for engine and interface plumbing.
5. Progress docs state that the reference app (the desktop application whose files DiaCraft opens and saves) was not inspected and
   that our output has not been opened in it by the project. Claims about it stay marked unverified.

## Assets (absolute)

An asset is any non-code material the project includes, ships, embeds, generates for publication or shows in its documentation:
icons, images, screenshots, logos, fonts, colour profiles, templates, themes, stencils, sample documents.

- Never add, trace, redraw or closely imitate anything from a Microsoft, Adobe, Avid or Autodesk product, installer, website, document
  or package: their stencils, templates, icons and fonts (Calibri, Cambria, Segoe, Aptos, Wingdings) included.
- Prefer art generated in code. Standard notation (flowchart, BPMN, UML, electrical) is drawn from the public standards. Other assets
  must be original, public domain or CC0, OSI-licensed, or redistributable Creative Commons, with proof of licence from the original
  source.
- Every asset file has a row in `ATTRIBUTION.md` (path, author, source URL and version, licence, use), and licence notices go in
  `NOTICE`. `cargo xtask assets` fails on an unattributed file. If provenance is unclear, do not add it.
- Fonts live in `storytold/craft-fonts`, never in this repository. They are an optional build input through `CRAFT_FONTS_DIR`.
  Everything must build, test and run without them. Self-hosted web fonts under an open licence (SIL OFL or similar) are allowed for
  the website only, with an attribution row and the licence text in the repository.
- No ArtCraft name, logo or link appears anywhere, and no `ai.storyteller` id. The app id is `app.diacraft`.
- Another vendor's product is named only in the file-type labels, the lists of the formats DiaCraft opens and saves, and the trademark
  notice, each an exact text of `PRODUCT_USES` in `xtask/src/brands.rs` that `cargo xtask brands` holds them to. Our own names, comments
  and other words stay neutral: where a comparison is needed, say "the reference app".
- The website's pages (the `.html` files of `site/`) may name the product the VSDX types are named after anywhere, to help searches,
  and `cargo xtask brands` and `cargo xtask site` let them (`SITE_PAGE_NAMES`). Every claim a page makes must be true of DiaCraft. The
  company's name and other vendors' names stay in the trademark notice, and a page's file name, the site's other files and the rest of
  the repository keep the rule above.
- Never commit files made by another diagramming application, and never commit large binary fixtures. Test inputs are generated by our
  code or written by hand.

## Never crash

People trust DiaCraft with their drawings; a crash loses work. This outranks feature work.

- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` in shipped code, and no `unsafe`. Lints sit in
  `[workspace.lints]` and in every crate and binary root:
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
  then `#![forbid(unsafe_code)]`. Every manifest has `[lints] workspace = true`.
- Fallible work returns `Result` or `Option`. Failures reach the user as an error message.
- Files, pasted data, control and MCP messages and command params are hostile: `.get()` not `[i]`, checked arithmetic, no NaN casts,
  capped allocations and loop counts, bounded recursion. A reader charges what it holds against a memory budget before it makes it.
- Don't silence errors to satisfy a lint. `let _ =` or `.ok()` only where ignoring is right, with a comment.
- `diacraft_engine::guard` catches panics at every entry point and rolls the document back. It is a safety net for bugs. WASM aborts
  on panic, so the web build has no net.
- Every crash fix lands with a regression test. Importers, parsers and protocol methods are fuzzed.
- Tests may unwrap and panic.

## Architecture

| Layer | Crates |
|---|---|
| L0 | `units`, `geom`, `color`, `formula`; standalone `container` (bounded zip and XML, for OPC and ODF packages), `opc` (on `container`), `metafile`, `dxf`, `data` |
| L1 | `doc`, `fonts` |
| L2 | `calc`, `text`, `pathops`, `layout`, `plugins`, `sheetxml` (the ShapeSheet XML codec both file formats use, and the whole-model codec) |
| L3 | `render`, `vsdx`, `odg`, `native` (`.diacraft`), `vsd`, `vdx`, `svg`, `pdf`, `pptx`, `emf`, `cadimport`, `interchange` |
| L4 | `tools` |
| L5 | `engine` |
| L6 | `ui-egui`, `mcp` |
| L7 | `apps/diacraft`, `apps/diacraft-cli`, `apps/diacraft-web`, `xtask` |

- `cargo xtask layers` enforces this. A crate in the table with no folder in `crates/` is planned and not built yet
  (`docs/architecture.md` lists what exists). There is no `diacraft-pathops`: the shape operations run on the shared `craft-pathops`.
  `testkit` is test support: it sits at L6 for this rule, and other crates use it only as a dev-dependency. Nothing below L6 depends on
  egui, eframe, winit or rfd.
- New workspace crates include `[lints] workspace = true` in `Cargo.toml`. Crate roots declare the lint attributes above before
  modules or other items; `//!` documentation may precede them.
- **The document model is the ShapeSheet.** Pages, masters and shapes hold sections of rows of cells; a cell holds a formula and a
  cached value with a unit. Do not add a second source of truth. A format's construct that cells can only approximate is held beside
  its stand-in cells, and used only while they are unchanged: the cells always win.
- **Edits are cell writes.** No command may rewrite or re-import the whole document.
- **`.diacraft` is DiaCraft's own format, and the default** ([`docs/diacraft-format.md`](docs/diacraft-format.md)). It holds the whole
  model, a superset of what VSDX and OpenDocument drawings hold, so a drawing that came from either is written back to it with the same
  content. It never stores the original file. **VSDX and `.odg` stay first-class:** each is read and written as faithfully as the
  other, and VSDX is the priority reference. A file is saved in its own format unless the user picks another, and a save in its own
  format splices into the file. In a VSDX or `.odg` file, DiaCraft's data goes in a namespaced extension, and holds only what that
  format cannot say. Unknown parts and attributes survive a save, and formulas and cached values are kept verbatim.
- **Everything is a command** in `crates/engine/src/cmd/*`: id, label, ribbon place, shortcut, params doc, `enabled`, `run`, plus
  tests. Menus, CLI, control channel and MCP reach the same registry. Programmatic calls never open dialogs.
- **The UI is thin:** panels read engine state and act through commands. Colours come from theme tokens.
- **Rust only**, no handwritten JS or TS, no webviews. **Never break WASM** (`cargo xtask wasm`). The one C library is the mimalloc
  allocator of the desktop app and the CLI: it is not built for the web.
- **Time goes through `jiff`** (workspace dependency). Never hand-roll UTC or calendar conversion (days-to-date algorithms, leap-year
  math); a crate owns only its output format string. The one exception is range extension: past the years jiff can represent (-9999
  to 9999), shift the date by whole 400-year Gregorian cycles (146,097 days, after which the calendar repeats), let jiff compute the
  month and day, and add the cycles back to the year.

## Evidence

Nobody on the project can run the reference app. Evidence comes from our own tests, independent readers such as LibreOffice used as a
black box, and round-trip stability. Each checklist row records which of these it rests on. Grade by behaviour, not by whether a menu
item exists. Scores err low and each number is marked measured or estimated.

## Tests, gates and commits

- Write the failing test first, watch it fail, write the code, watch it pass. No test asserts elapsed time: count work, and put
  real-time checks behind `#[ignore]`. A refusal test asserts the words, not only `is_err()`.
- **Before asking to merge,** run: the build, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the
  tests of the changed crates and of the crates that depend on them (`cargo tree -i <crate> -e normal --workspace`).
  `cargo xtask ci` runs the whole gate; CI runs it on every pull request, and the maintainer runs it in full before anything is
  pushed to `main`.
- **Kani** (`cargo xtask kani`) proves properties wherever it can handle the code: integer code, byte code, and small functions of 1
  to 3 floats. Harnesses live in a crate's `src/kani_proofs.rs`, behind `#[cfg(kani)]`. A proof that fails on our code is a bug.
- New or changed fuzz targets get a run of 10 minutes before they merge (`docs/development.md`, "Fuzzing").
- One change per commit, its message starting with what it changes. Add explicit paths only, never `git add -A`.

## Running and looking at the app

- `cargo run --release -p diacraft -- --control 7997`. Pick another port if the log says it failed to bind.
- The window opens with a splash popup for 4 s that takes the first click or key. Set `DIACRAFT_NO_SPLASH=1` when a script drives the
  window by clicks or keys.
- Drive it with JSON lines on `127.0.0.1:7997`:
  `{"id":1,"method":"engine.execute","params":{"command":"file.new","params":{}}}`, then
  `{"id":2,"method":"ui.screenshot","params":{"path":"/absolute/shot.png"}}`.
  Methods are in `docs/control-protocol.md`.
- **For UI work, look at the result:** take a screenshot and read the PNG.
- MCP: `diacraft-cli mcp` (`docs/mcp.md`). CLI: `docs/cli.md`.

## Progress docs

Follow the templates in `docs/progress-doc-templates.md`. A status line, `> **Last reviewed:** YYYY-MM-DD` under the H1, is kept by
the ten scored progress docs alone, and says only when their scores were last checked against evidence. The other docs have none, and
no status line holds an edit date, a list of changes or a history: git records when a doc changed and what changed. Every
doc ends with its revision table. When a change lands, update in the same change the docs it touches: the area row in
`docs/target-app-parity.md`, the entry in `docs/gaps.md`, the checklist row, the milestone row in `docs/roadmap.md`, and `ROADMAP.md`
when a headline number moves. Keep the README's status in step. `cargo xtask docs` and `cargo xtask parity --check` check part of this.

## Contributor credits

`contributors/people.toml` holds names people chose for themselves. Add at most your own entry. Never add, edit, guess or copy anyone
else's name.

## CI workflows

`.github/workflows/ci.yml` runs `cargo xtask ci` on Linux and builds and smoke tests the app and the command line on Windows and macOS;
`.github/workflows/release.yml` builds the downloads of a pushed `v*` tag into a draft release and deploys the website and the web
app (`docs/releasing.md`); `.github/workflows/deploy-site.yml` deploys the two sites by hand between releases. Don't add or change CI
in a feature or fix change.

CodeRabbit reviews pull requests to this repository, as the `.coderabbit.yaml` at the root sets it up. **Its review is advice only and
never blocks a merge.** A maintainer checks each finding. A real one is fixed in the source tree this repository is published from and
published again, not on the public branch; a finding that is not real is dropped.
