//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std, serde_json, toml, roxmltree and sha2). External tools (`cargo`, `git`, and `trunk` for `site --web`) run
//! through `std::process::Command`.
//! The checks live in the library (`lib.rs` and its modules); this file wires them to commands and
//! to the `ci` step list.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(kani)]
mod kani_proofs;
mod site;

use std::process::ExitCode;

use xtask::ci::{Outcome, Step, run_steps};
use xtask::{
    assets, brands, cargo, deny, docs_check, fuzzcheck, has_cargo_tool, help, kani, langdata, layers, licenses, mcp, metadata, parity, root, run,
    stats, version,
};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci              fmt --check, clippy -D warnings, fuzz check, kani check, test, doctest, assets, brands, docs, help check,
                  site, deny, licenses --check, parity --check, layers, wasm (stops at the first failure; prints a `CI summary:` block)
  test            the workspace tests (cargo nextest when installed, else cargo test), then the doctests
  fuzz-check      type-check every fuzz crate (a folder named fuzz, a `cargo-fuzz = true` line, or a dependency on libfuzzer-sys) with
                  `cargo check --all-targets --all-features` and `--cfg fuzzing` on the stable toolchain: no nightly, no sanitizer, no C++
                  build of libFuzzer (an empty stand-in library is named by CUSTOM_LIBFUZZER_PATH). Each crate's Cargo.lock is made from
                  the workspace's, again when that changes, and completed offline first (online only when that fails). Fails, naming the
                  target, when one does not compile, when a file in fuzz_targets is not a [[bin]] of its manifest, and for any Cargo.toml
                  that no step builds. Builds into <target>/fuzz-check
  kani [-j N]     prove every Kani harness (a crate's src/kani_proofs.rs, behind `#[cfg(kani)]`) with `cargo kani` 0.68.0, N at a time (4 unless
                  given), each with a 300 s limit. Fails, naming the harness, on a failure, a timeout, or a `kani::cover!` no input reaches; fails
                  without Kani 0.68.0 (cargo install --locked kani-verifier --version 0.68.0, then cargo kani setup). Minutes: not part of ci,
                  which only compiles the harnesses (the `kani check` step: `cargo kani --only-codegen`, skipped without Kani 0.68.0)
  help check | help --write
                  hold the help tree (`help/`) to the rules of its format with the parser the program reads it with: each topic reads within its limits and its
                  header is complete, ids are unique, named by their file, and none that was published is lost, every `topic:` link and `#anchor` resolves, every
                  `ctl:` names a ribbon control and every `cmd:` a registered command, every topic is in a section or `unlisted`, the tips table and the help map name
                  only controls, commands, places and topics that exist, the sites a link may open are the site's own, and the words follow the rules of `brands`;
                  each problem is printed as `file:line: what`. `--write` runs that check and writes what is generated from the tree (nothing yet). `help` alone is
                  this text
  langdata --cldr <DIR> --icu <DIR> [--i18n-source <FILE>] [--check]
                  write the four generated tables of `diacraft_doc::lang` (crates/doc/src/lang: lcid.rs, names.rs, quotes.rs, scripts.rs): LCID numbers and
                  language tags from ICU4C's locmap.cpp, English names of languages, quotation marks and likely scripts from Unicode CLDR's JSON, both data under the
                  Unicode License V3 at the releases pinned in xtask/src/langdata.rs. It reads folders you made (docs/development.md) and fetches nothing; each
                  source is checked first (CLDR version and licence, ICU licence, locmap.cpp's SHA-256). --i18n-source refuses a CLDR release other than the one
                  craft-i18n's data record names; --check writes nothing and fails naming each committed table that is not what the sources give. Not part of ci
  layers          enforce the crate dependency layering (design section 6)
  assets          check that every asset file (anything under assets/ or docs/images/, and any file whose extension is an image,
                  icon, font, colour-profile, audio, video, PDF, CAD, metafile or diagram-document format) has a row in ATTRIBUTION.md
  brands          check that user-visible text (labels, params docs, MCP, packaging) names no other vendor's products; fails on
                  any file that names another project's brand, link or app id (a few listed lines excepted)
  docs            check ROADMAP.md and docs/*.md for repeated table rows and stacked status lines, and that only the ten scored docs
                  have a status line, which holds their review date alone (`> **Last reviewed:** YYYY-MM-DD`): no `Last updated`
                  date, no `**Change:**` list and no history, which git records
  site [--write|--release|--web [--dist DIR] [--out DIR] [--app-out DIR] [--app-url URL]]
                  check the static website; --write refreshes generated links and metadata (the pages' links to the app take app_url of site.json); --release
                  also checks release settings; --web also makes the two sites the website is: the pages (site/ with its links to the app at app_url, or at
                  --app-url / DIACRAFT_APP_URL for a preview on this machine) in the pages' folder (--out, dist/site when none), and the web app
                  (`trunk build --release` in apps/diacraft-web, or the finished build in DIR) at the root of a folder of its own (--app-out, dist/app when
                  none) with its own _headers; and checks that: every file and link of the app, each file under the 25 MiB Cloudflare Pages
                  allows and the budget below it, no inline script or style, no shared memory, no network call in the glue, the registry's proof of the domain
                  at .well-known/mcp-registry-auth, and each folder's _headers
  deploy-check <BASE_URL> [<BASE_URL>]
                  after a deploy: for each site (the pages, then the app) fetch the start page, every file it loads and the pictures from BASE_URL with
                  `Accept-Encoding: br, gzip` (through curl), and say for each the status, Cache-Control, Content-Encoding and ETag, and whether a
                  second request with If-None-Match gets 304. Fails on a status that is not 200 (but for the 308 Pages sends `/x.html` to `/x` with,
                  which it follows once) and on a wrong Cache-Control (a file named by its
                  hash must be immutable for a year, any other must not be kept; for the app's site also the proof at /.well-known/mcp-registry-auth: 200, text/plain,
                  a record); warns when a .wasm part or another text file comes back
                  uncompressed, or has no ETag. BASE_URL is https://host, or http://host:port for a server of your own
  web-dpr [DIR]   the built web app (DIR, or dist/app from `site --web`) in a headless Chrome or Chromium (the program on the PATH, or CHROME; extra
                  arguments in CHROME_ARGS) at device scale factors of 1, 1.5, 2 and 3, a phone-sized page at 3, and a running app that is resized: serves
                  DIR on a loopback port, drives the browser over the DevTools protocol (`Emulation.setDeviceMetricsOverride`), and fails when the canvas
                  has not its CSS size times the ratio in pixels, which is a window laid out for part of the page. Needs the browser, so it is no step of ci
  deny            cargo deny check licenses bans sources, then advisories (deny.toml; fails when cargo-deny is not installed; with no network
                  the advisories run against the copy of the database on the machine, and fail when there is none)
  licenses [--check]
                  write THIRD-PARTY-LICENSES.txt: every crate `diacraft` and `diacraft-cli` are built from on Linux, Windows and macOS (cargo tree and
                  cargo metadata per target), with its licence, the licence DiaCraft uses it under (MIT where offered) and each licence and NOTICE
                  text once, the standard text where a crate ships none; and THIRD-PARTY-LICENSES-WEB.txt: the same for the web build (`diacraft-web`
                  on wasm32-unknown-unknown), which the web app's window shows; --check fails when a crate has no text, a licence or a licence file is
                  outside deny.toml's list, a crate compiles C code that xtask/src/licenses/policy.rs does not name, or a file is out of date. Needs
                  the sources of every platform's crates, which cargo fetches (a network, once)
  mcp-server-json <SHA256SUMS.txt> [--out <server.json>]
                  write the entry of the MCP server in the official MCP Registry (name app.diacraft/diacraft): one mcpb package for each of the six
                  bundles of the release (`diacraft-mcp-<version>-<system>-<cpu>.mcpb`), by its address in the release and its SHA-256, read from the
                  release's SHA256SUMS.txt; to the file --out names, or to the standard output. Run it on the checkout of the release's tag, after
                  the release is published, then `mcp-publisher publish` (docs/releasing.md). Fails on sums that miss a bundle, name one twice or
                  are of another release. It publishes nothing
  parity [--check]
                  recompute the summary in docs/parity-checklist.md; --check fails when it is out of date, when the
                  file does not exist, when a line under `## Rows` (to the next `#` or `##` heading) that holds a `|`
                  is not a row it can read (or a header with its separator), and when there are no rows
  wasm            cargo clippy -D warnings --target wasm32-unknown-unknown for the crates of layer 5 or lower, ui-egui
                  and the web app (\"nothing to check\" until they exist)
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  version [set X.Y.Z[-pre]]
                  print the workspace version, or set it (Cargo.toml and Cargo.lock)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("ci") => cmd_ci(),
        Some("test") => cmd_test().and_then(|()| cmd_doctests()),
        Some("fuzz-check") => report("fuzz check", fuzzcheck::run()),
        Some("kani") => kani::parse_jobs(&rest).and_then(|jobs| report("kani", kani::prove(jobs))),
        Some("langdata") => langdata::run(&root(), &rest),
        Some("layers") => cmd_layers(),
        Some("assets") => assets::run(&root()),
        Some("brands") => brands::run(&root()),
        Some("docs") => docs_check::run(&root()),
        Some("site") => site::run(&root().join("site"), &rest),
        Some("deploy-check") => site::deploy_check(&rest),
        Some("web-dpr") => site::web_dpr(&root(), &rest),
        Some("deny") => report("deny", deny::run()),
        Some("licenses") => licenses::run(&root(), &rest),
        Some("mcp-server-json") => mcp::run(&root(), &rest),
        Some("parity") => cmd_parity(&rest),
        Some("wasm") => report("wasm", cmd_wasm()),
        Some("stats") => cmd_stats(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("help") if !rest.is_empty() => help::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Prints why a command that did not apply was skipped. A skip is not a failure.
fn report(name: &str, result: Result<Outcome, String>) -> Result<(), String> {
    match result? {
        Outcome::Ok => {}
        Outcome::Skipped(why) => println!("{name}: skipped ({why})"),
    }
    Ok(())
}

fn cmd_parity(rest: &[&str]) -> Result<(), String> {
    match rest {
        [] => parity::run(&root()),
        ["--check"] => report("parity", parity::check(&root())),
        _ => Err("usage: cargo xtask parity [--check]".into()),
    }
}

fn cmd_stats(rest: &[&str]) -> Result<(), String> {
    match rest {
        [] => stats::run(&root(), false),
        ["--exact"] => stats::run(&root(), true),
        _ => Err("usage: cargo xtask stats [--exact]".into()),
    }
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (design section 6)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// One `cargo clippy -D warnings` over the whole wasm set: the crates share one build, and `--keep-going` reports
/// every crate that fails rather than only the first. Clippy and not `cargo check`: the web app's browser code is
/// compiled by nothing else, WebAssembly aborts on a panic, and `check` runs none of the lints that forbid `unwrap`,
/// `expect` and `panic!` there. Skipped while no such crate exists.
fn cmd_wasm() -> Result<Outcome, String> {
    let set = layers::wasm_set(&layers::from_metadata(&metadata()?)?);
    if set.is_empty() {
        println!("wasm: nothing to check (no crate of layer 5 or lower, ui-egui or web app exists yet)");
        return Ok(Outcome::Skipped("nothing to check".into()));
    }
    let mut c = cargo();
    c.args(["clippy", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for pkg in &set {
        c.args(["-p", pkg]);
    }
    c.args(["--", "-D", "warnings"]);
    run(c, &format!("cargo clippy --target wasm32-unknown-unknown --keep-going ({} crates: {}) -- -D warnings", set.len(), set.join(", ")))?;
    println!("wasm32-unknown-unknown clippy: ok ({} crates: {})", set.len(), set.join(", "));
    Ok(Outcome::Ok)
}

/// The workspace's unit and integration tests, without examples (clippy still checks those).
/// nextest runs each test in its own process, in parallel across all test binaries.
fn cmd_test() -> Result<(), String> {
    const TARGETS: [&str; 3] = ["--lib", "--bins", "--tests"];
    let mut c = cargo();
    if has_cargo_tool("nextest") {
        c.args(["nextest", "run", "--workspace"]).args(TARGETS);
        run(c, "cargo nextest run --workspace --lib --bins --tests")
    } else {
        eprintln!("note: cargo-nextest is not installed; running cargo test (cargo install cargo-nextest --locked --version 0.9.150)");
        c.args(["test", "--workspace"]).args(TARGETS);
        run(c, "cargo test --workspace --lib --bins --tests")
    }
}

/// The doctests, which nextest doesn't run.
fn cmd_doctests() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

/// A step that runs one cargo command.
fn cargo_step(args: &'static [&'static str]) -> Result<Outcome, String> {
    let mut c = cargo();
    c.args(args);
    run(c, &format!("cargo {}", args.join(" ")))?;
    Ok(Outcome::Ok)
}

/// `cargo xtask ci`: each step in turn, stopping at the first that fails. The summary lists every
/// step as ok or skip (with the reason); after a failure it names the step that failed and those that
/// did not run.
fn cmd_ci() -> Result<(), String> {
    let steps: Vec<Step> = vec![
        ("fmt", Box::new(|| cargo_step(&["fmt", "--all", "--", "--check"]))),
        ("clippy", Box::new(|| cargo_step(&["clippy", "--workspace", "--all-targets", "--keep-going", "--", "-D", "warnings"]))),
        ("fuzz check", Box::new(fuzzcheck::run)),
        ("kani check", Box::new(kani::check)),
        ("test", Box::new(|| cmd_test().map(|()| Outcome::Ok))),
        ("doctest", Box::new(|| cmd_doctests().map(|()| Outcome::Ok))),
        ("assets", Box::new(|| assets::run(&root()).map(|()| Outcome::Ok))),
        ("brands", Box::new(|| brands::run(&root()).map(|()| Outcome::Ok))),
        ("docs", Box::new(|| docs_check::run(&root()).map(|()| Outcome::Ok))),
        ("help check", Box::new(|| help::check::run(&root()).map(|()| Outcome::Ok))),
        ("site", Box::new(|| site::run(&root().join("site"), &[]).map(|()| Outcome::Ok))),
        ("deny", Box::new(deny::run)),
        ("licenses --check", Box::new(|| licenses::run(&root(), &["--check"]).map(|()| Outcome::Ok))),
        ("parity --check", Box::new(|| parity::check(&root()))),
        ("layers", Box::new(|| cmd_layers().map(|()| Outcome::Ok))),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let (text, result) = run_steps(&steps);
    print!("{text}");
    result
}
