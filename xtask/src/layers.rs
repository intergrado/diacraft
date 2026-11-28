//! Dependency layering rules (AGENTS.md, "Architecture").
//!
//! The rule engine works on a small, metadata-independent model so it can be
//! unit-tested; `from_metadata` builds that model from `cargo metadata`.

use serde_json::Value;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Regular layered crate.
    Layer(u8),
    /// Layer 0, and additionally may depend on no workspace crate at all, but another standalone crate that an intra-layer chain
    /// orders before it (`INTRA_LAYER_ORDER`).
    Standalone,
    /// Test tooling: may depend on anything up to L5 (it sits at L6 for rule
    /// purposes); other crates may use it only as a dev-dependency.
    Testkit,
    /// Binaries and build tooling: exempt from the rules.
    Exempt,
}

impl Class {
    fn layer(self) -> Option<u8> {
        match self {
            Class::Layer(l) => Some(l),
            Class::Standalone => Some(0),
            Class::Testkit => Some(6),
            Class::Exempt => None,
        }
    }
}

/// The layering table (design section 6). Names are package names without the `diacraft-` prefix.
/// Crates added later are appended; the builder may split or merge crates, and the table follows.
pub const TABLE: &[(&str, Class)] = &[
    // L0: unit-typed values, curves, theme colours, the formula parser and printer
    ("units", Class::Layer(0)),
    ("geom", Class::Layer(0)),
    ("color", Class::Layer(0)),
    ("formula", Class::Layer(0)),
    // L0, standalone: the bounded ZIP and XML layer of both package formats, OPC packages, EMF and WMF, DXF
    // tags, spreadsheet readers for data linking
    ("container", Class::Standalone),
    ("opc", Class::Standalone),
    ("metafile", Class::Standalone),
    ("dxf", Class::Standalone),
    ("data", Class::Standalone),
    // The person's settings: `config.toml` through craft-settings and the files beside it, bounded and named. Below the engine so that its hostile-input parts have a fuzz
    // target of their own, and the engine, the window and the command line reach it through one crate.
    ("prefs", Class::Standalone),
    // The help layer: the format of the in-app help, its parser, catalogs, ranking and tables, with no DiaCraft type, so that it can move to craft-libs as `craft-help`.
    ("help", Class::Standalone),
    // L1: the document model and the font database
    ("doc", Class::Layer(1)),
    ("fonts", Class::Layer(1)),
    // L2: recalculation, text layout, shape operations, graph layout, connector routing, plug-ins, the ShapeSheet XML codec
    ("calc", Class::Layer(2)),
    ("text", Class::Layer(2)),
    ("pathops", Class::Layer(2)),
    ("layout", Class::Layer(2)),
    ("route", Class::Layer(2)),
    ("plugins", Class::Layer(2)),
    // The ShapeSheet XML codec carved out of `vsdx`: the cells, sections and rows of a sheet as XML, for the VSDX package and the OpenDocument
    // one. It uses the document model (layer 1) and the container's XML tree (layer 0, standalone), so a format crate of layer 3 uses it.
    ("sheetxml", Class::Layer(2)),
    // L3: rendering and every file format
    ("render", Class::Layer(3)),
    ("vsdx", Class::Layer(3)),
    // The OpenDocument drawing (`.odg`): a file format like `vsdx`, so layer 3. It reads the package through the container
    // crate (layer 0); its reader and writer will use the model of layer 1 and never `vsdx`, a crate of its own layer.
    ("odg", Class::Layer(3)),
    // DiaCraft's own format (`.diacraft`): the package of the model. It uses the whole-model codec of layer 2 (`sheetxml`), the model (layer 1) and the
    // container (layer 0), and never `vsdx` or `odg`, crates of its own layer: no chain inside the layer.
    ("native", Class::Layer(3)),
    // The draw.io reader: a file format like the rest of this layer, so layer 3. It finds a drawing in a file, decodes its pages and reads them into the model
    // (layer 1) through the container (layer 0, standalone: the bounded XML tree and the bounded inflate), and never uses `vsdx`, `odg` or `native`, crates of its own layer: no chain
    // inside the layer. Import only: the engine opens these files, and nothing writes them.
    ("mxfile", Class::Layer(3)),
    ("vsd", Class::Layer(3)),
    ("vdx", Class::Layer(3)),
    ("svg", Class::Layer(3)),
    ("pdf", Class::Layer(3)),
    ("pptx", Class::Layer(3)),
    ("emf", Class::Layer(3)),
    ("cadimport", Class::Layer(3)),
    ("interchange", Class::Layer(3)),
    // L4: tool state machines
    ("tools", Class::Layer(4)),
    // L5: sessions, undo, the command registry, guard, recovery, file I/O, macros
    ("engine", Class::Layer(5)),
    // L6: the egui interface and the MCP server
    ("ui-egui", Class::Layer(6)),
    ("mcp", Class::Layer(6)),
    ("testkit", Class::Testkit),
    // L7: the desktop app, the CLI, the web build and tooling
    ("diacraft", Class::Exempt),
    ("cli", Class::Exempt),
    ("web", Class::Exempt),
    ("xtask", Class::Exempt),
];

/// Explicit orderings *within* a layer (earlier may be used by later). A crate may not use another of
/// its own layer until the design says it should. Add a chain here, with the reason, when it does. A chain names crates as
/// `TABLE` does (no `diacraft-` prefix), all of one layer, none twice, and no two chains order a pair both ways: `chain_problems`
/// checks that, as part of `check`, so a chain that would allow nothing, or too much, fails the layers step. Standalone crates count
/// as layer 0, and a chain may order them among themselves only: a standalone crate then uses the one before it and no other
/// workspace crate, and a layered crate is never in a chain with one.
///
/// `units` before `formula`: the formula crate keeps a number literal's unit as `diacraft_units::Unit`
/// and reads the suffixes with `parse_unit`, so the unit table lives in one place.
///
/// `render` before `svg` and before `pdf`: the walk of a page is written once, in `render`, and an export writer implements its `Target`
/// trait. The two writers do not use each other.
///
/// `text` before `calc`: `TEXTWIDTH` and `TEXTHEIGHT` are the size of the text laid out, so the evaluator lays it out with the text crate and does
/// not write a second layout. The text crate does not use `calc`.
///
/// `container` before `opc`: the bounded ZIP reader and writer and the bounded XML tree are shared by the VSDX package and the OpenDocument
/// package, so they live in one standalone crate that `opc` reads and writes its archives through. The container
/// crate knows nothing of OPC.
pub const INTRA_LAYER_ORDER: &[&[&str]] = &[&["units", "formula"], &["render", "svg"], &["render", "pdf"], &["text", "calc"], &["container", "opc"]];

fn intra_layer_allowed(from: &str, to: &str, chains: &[&[&str]]) -> bool {
    let (from, to) = (short_name(from), short_name(to));
    chains.iter().any(|chain| match (chain.iter().position(|n| *n == from), chain.iter().position(|n| *n == to)) {
        (Some(f), Some(t)) => t < f,
        _ => false,
    })
}

/// External crates that constitute a UI toolkit / windowing dependency. `craft-window` is in because
/// the workspace turns on its `egui` feature, which pulls egui into whoever depends on it.
pub const UI_CRATES: &[&str] = &["egui", "eframe", "egui_extras", "winit", "egui_kittest", "craft-ui", "craft-keymap", "craft-window", "rfd"];

/// Name prefixes of UI toolkit families: `egui` (egui-winit, egui-wgpu, egui_glow, egui_inspection,
/// egui_system_font_provider, egui_demo_lib, ...), `epaint` (epaint_default_fonts, ...) and `wgpu`
/// (wgpu-core, wgpu-hal, wgpu-types, ...). A prefix also matches a longer name that merely starts
/// with it, which can only report more.
pub const UI_PREFIXES: &[&str] = &["egui", "epaint", "wgpu"];

/// First layer allowed to use UI crates.
pub const UI_MIN_LAYER: u8 = 6;

/// A crate named like a UI toolkit that holds data and no toolkit, and the one crate below L6 that may use it, as
/// `(workspace crate without the prefix, dependency)`. `epaint_default_fonts` is the bytes of two typefaces (Hack and Ubuntu Light,
/// and three more behind a feature this workspace leaves off: NOTICE, and the lock test in `tests/it/workspace.rs`), and nothing in it
/// draws or opens a window. The `fonts` crate takes the first two from it by name, so that the font database has its last-resort faces
/// on every target, the web build included, with no dependency on layer 6. Add a pair here only for a data crate, with its reason.
pub const UI_EXCEPTIONS: &[(&str, &str)] = &[("fonts", "epaint_default_fonts")];

/// Is `dep` one that `krate` may use although it is named like a UI toolkit ([`UI_EXCEPTIONS`])?
fn ui_exception(krate: &str, dep: &str) -> bool {
    UI_EXCEPTIONS.contains(&(short_name(krate), dep))
}

pub fn short_name(pkg: &str) -> &str {
    pkg.strip_prefix("diacraft-").unwrap_or(pkg)
}

pub fn classify(pkg: &str) -> Option<Class> {
    let s = short_name(pkg);
    TABLE.iter().find(|(n, _)| *n == s).map(|(_, c)| *c)
}

fn is_ui_crate(name: &str) -> bool {
    UI_CRATES.contains(&name) || UI_PREFIXES.iter().any(|p| name.starts_with(p))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub kind: DepKind,
    /// `true` if the dependency is a workspace member.
    pub workspace: bool,
}

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Unregistered { krate: String },
    Upward { krate: String, dep: String, from: u8, to: u8, kind: DepKind },
    StandaloneHasWorkspaceDep { krate: String, dep: String },
    TestkitAsNormalDep { krate: String },
    UiBelowL6 { krate: String, dep: String, layer: u8 },
    BadChain { chain: String, reason: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::Unregistered { krate } => {
                write!(f, "{krate}: unknown workspace crate; register it in xtask/src/layers.rs TABLE (design section 6)")
            }
            Violation::Upward { krate, dep, from, to, kind } => {
                write!(f, "{krate} (L{from}) -> {dep} (L{to}) [{kind:?}]: may only depend on strictly lower layers")
            }
            Violation::StandaloneHasWorkspaceDep { krate, dep } => {
                write!(f, "{krate}: standalone crate must not depend on workspace crate {dep}")
            }
            Violation::TestkitAsNormalDep { krate } => {
                write!(f, "{krate}: diacraft-testkit may only be a dev-dependency")
            }
            Violation::UiBelowL6 { krate, dep, layer } => {
                write!(f, "{krate} (L{layer}) depends on UI crate `{dep}`; UI toolkits are only allowed in L6+")
            }
            Violation::BadChain { chain, reason } => {
                write!(f, "intra-layer chain [{chain}]: {reason} (INTRA_LAYER_ORDER, xtask/src/layers.rs)")
            }
        }
    }
}

/// What is wrong with a list of intra-layer chains, one violation for each thing. A chain that orders nothing is a mistake that
/// would otherwise pass: a crate not in `TABLE` or not layered, crates of two layers, a name twice, fewer than two names, and two
/// chains (or a ring of them) that order a pair both ways, which would allow a dependency in both directions.
pub fn chain_problems(chains: &[&[&str]]) -> Vec<Violation> {
    let mut out = Vec::new();
    // Pairs `(a, b)`: some chain has `a` before `b`. Only chains with nothing wrong in them count.
    let mut before: Vec<(&str, &str)> = Vec::new();
    for chain in chains {
        let reasons = chain_reasons(chain);
        if reasons.is_empty() {
            for (i, a) in chain.iter().enumerate() {
                before.extend(chain.iter().skip(i + 1).map(|b| (*a, *b)));
            }
        }
        let label = chain.join(" before ");
        out.extend(reasons.into_iter().map(|reason| Violation::BadChain { chain: label.clone(), reason }));
    }
    // Close the pairs under "before": a pair in both orders is a cycle.
    loop {
        let more: Vec<(&str, &str)> = before
            .iter()
            .flat_map(|(a, b)| before.iter().filter(move |(c, _)| c == b).map(move |(_, d)| (*a, *d)))
            .filter(|p| !before.contains(p))
            .collect();
        if more.is_empty() {
            break;
        }
        for pair in more {
            if !before.contains(&pair) {
                before.push(pair);
            }
        }
    }
    let mut both: Vec<(&str, &str)> = before.iter().filter(|(a, b)| a < b && before.contains(&(*b, *a))).copied().collect();
    both.sort_unstable();
    for (a, b) in both {
        out.push(Violation::BadChain {
            chain: format!("{a} before {b}"),
            reason: format!("`{a}` is ordered before `{b}` and `{b}` before `{a}` by the chains together: each may use the other, both ways"),
        });
    }
    out
}

/// What is wrong with one chain by itself.
fn chain_reasons(chain: &[&str]) -> Vec<String> {
    let mut reasons = Vec::new();
    if chain.len() < 2 {
        reasons.push("a chain orders at least two crates, and this orders nothing".to_string());
    }
    let mut layers = Vec::new();
    let mut standalone = Vec::new();
    for (i, name) in chain.iter().enumerate() {
        // A chain names crates as `TABLE` does, without the `diacraft-` prefix: `intra_layer_allowed` compares them as written.
        match TABLE.iter().find(|(n, _)| n == name) {
            None => reasons.push(format!("`{name}` is not in TABLE (names are without the `diacraft-` prefix)")),
            Some((_, Class::Layer(l))) => layers.push(*l),
            Some((_, Class::Standalone)) => standalone.push(*name),
            Some(_) => reasons.push(format!(
                "`{name}` is not a layered crate: only a `Class::Layer` crate, or a standalone one among standalone ones, has an order within its layer"
            )),
        }
        if chain.iter().take(i).any(|earlier| earlier == name) {
            reasons.push(format!("`{name}` is in the chain twice"));
        }
    }
    // Standalone crates count as layer 0 and are ordered among themselves: one in a chain with a layered crate would let a standalone crate
    // use a layered one, or a layered crate of layer 0 use a standalone one.
    if !layers.is_empty() {
        for name in standalone {
            reasons.push(format!(
                "`{name}` is not a layered crate, and the chain holds layered ones: a standalone crate is ordered only among standalone ones"
            ));
        }
    }
    layers.sort_unstable();
    layers.dedup();
    if layers.len() > 1 {
        reasons.push(format!("the crates are of different layers ({layers:?}): a chain orders the crates of one layer"));
    }
    reasons
}

/// Check all rules. Returns violations sorted for stable output.
pub fn check(crates: &[Crate]) -> Vec<Violation> {
    check_with_chains(crates, INTRA_LAYER_ORDER)
}

/// [`check`] with the given intra-layer chains in place of `INTRA_LAYER_ORDER`.
pub fn check_with_chains(crates: &[Crate], chains: &[&[&str]]) -> Vec<Violation> {
    let mut out = chain_problems(chains);
    for c in crates {
        let Some(class) = classify(&c.name) else {
            out.push(Violation::Unregistered { krate: c.name.clone() });
            continue;
        };
        if class == Class::Exempt {
            continue;
        }
        let layer = class.layer().unwrap_or(0);
        for d in &c.deps {
            // Self dev-dependencies (e.g. to enable features in tests) are fine.
            if d.name == c.name {
                continue;
            }
            if d.workspace {
                if class == Class::Standalone {
                    // Another standalone crate that a chain orders before this one, and nothing else.
                    let ordered = classify(&d.name) == Some(Class::Standalone) && intra_layer_allowed(&c.name, &d.name, chains);
                    if !ordered {
                        out.push(Violation::StandaloneHasWorkspaceDep { krate: c.name.clone(), dep: d.name.clone() });
                    }
                    continue;
                }
                match classify(&d.name) {
                    // Unregistered deps are reported on their own entry.
                    None => {}
                    Some(Class::Testkit) if d.kind != DepKind::Dev => {
                        out.push(Violation::TestkitAsNormalDep { krate: c.name.clone() });
                    }
                    // The testkit as a dev-dependency is how tests use it.
                    Some(Class::Testkit) => {}
                    Some(dc) => {
                        let to = dc.layer().unwrap_or(u8::MAX);
                        if to >= layer && !(to == layer && intra_layer_allowed(&c.name, &d.name, chains)) {
                            out.push(Violation::Upward {
                                krate: c.name.clone(),
                                dep: d.name.clone(),
                                from: layer,
                                to: if to == u8::MAX { 7 } else { to },
                                kind: d.kind,
                            });
                        }
                    }
                }
            } else if layer < UI_MIN_LAYER && is_ui_crate(&d.name) && !ui_exception(&c.name, &d.name) {
                out.push(Violation::UiBelowL6 { krate: c.name.clone(), dep: d.name.clone(), layer });
            }
        }
    }
    out.sort_by_key(|v| v.to_string());
    out.dedup();
    out
}

/// Build the model from `cargo metadata --format-version 1 --no-deps`.
pub fn from_metadata(meta: &Value) -> Result<Vec<Crate>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let members: Vec<&str> = pkgs.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut out = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().ok_or("package without name")?.to_owned();
        let mut deps = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dname = d["name"].as_str().unwrap_or_default().to_owned();
            let kind = match d["kind"].as_str() {
                Some("dev") => DepKind::Dev,
                Some("build") => DepKind::Build,
                _ => DepKind::Normal,
            };
            let workspace = members.contains(&dname.as_str()) || d["path"].is_string();
            deps.push(Dep { name: dname, kind, workspace });
        }
        out.push(Crate { name, deps });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The packages that must build for wasm32: every crate of layer 5 or lower (standalone ones
/// included), the egui shell, and the web app (its wasm-only code is compiled by nothing else).
pub fn wasm_set(crates: &[Crate]) -> Vec<String> {
    crates
        .iter()
        .filter(|c| match classify(&c.name) {
            Some(Class::Layer(l)) => l <= 5 || short_name(&c.name) == "ui-egui",
            Some(Class::Standalone) => true,
            _ => short_name(&c.name) == "web",
        })
        .map(|c| c.name.clone())
        .collect()
}

pub fn describe(class: Option<Class>) -> String {
    match class {
        Some(Class::Layer(l)) => format!("L{l}"),
        Some(Class::Standalone) => "L0 standalone".into(),
        Some(Class::Testkit) => "testkit".into(),
        Some(Class::Exempt) => "exempt".into(),
        None => "UNREGISTERED".into(),
    }
}
