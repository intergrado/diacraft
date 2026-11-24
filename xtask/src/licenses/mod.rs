//! `cargo xtask licenses`: the notices that go with DiaCraft's programs.
//!
//! MIT, BSD, Apache-2.0, Zlib and the other permissive licences of the crates the programs are built from ask that their notices and licence texts travel with
//! the programs. This command makes two files at the root of the repository. `THIRD-PARTY-LICENSES.txt` lists every crate that `diacraft` and `diacraft-cli`
//! are built from, on Linux, Windows and macOS (two builds), with its licence, the licence DiaCraft uses it under, and every licence and notice text once; it is
//! kept in the repository, built into the desktop window's About panel, and put in every download (`packaging/package.sh`). `THIRD-PARTY-LICENSES-WEB.txt` is the
//! same for the web build, `diacraft-web` on `wasm32-unknown-unknown`, which runs in the browser from the website: the web app's licences window shows it, and
//! the app's page links it (`policy::OUTPUTS` says which platforms each file covers).
//!
//! * `cargo xtask licenses` writes both files.
//! * `cargo xtask licenses --check` fails when a crate has no licence text, when a crate's licence (or a licence file in it) is outside `deny.toml`'s list,
//!   when a crate compiles C code that `policy.rs` does not name, when a list of `policy.rs` is stale, or when a file in the repository is not what the
//!   lock says now. It reads `Cargo.lock` and the sources cargo has fetched (a platform's own crates are fetched by cargo, with a network, the first time).
//!
//! The pieces: [`gather`] asks cargo, [`assemble`] decides and collects, [`render`] writes, and [`policy`], [`spdx`], [`classify`] and [`files`] hold what
//! they need.

pub mod assemble;
pub mod classify;
pub mod files;
pub mod gather;
pub mod policy;
pub mod render;
pub mod spdx;

use std::path::Path;

use assemble::{Assembly, Inputs, assemble_with};
use policy::{OUTPUTS, Output};

/// The usage line.
const USAGE: &str = "usage: cargo xtask licenses [--check]";

/// Runs the command.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    match args {
        [] => write(root),
        ["--check"] => check(root),
        _ => Err(USAGE.to_owned()),
    }
}

/// The notices of the builds `inputs` holds, with the lists of `policy.rs` checked against them: the text, or the problems that stand in the way.
pub fn notices(inputs: &Inputs) -> Result<(String, Assembly), Vec<String>> {
    notices_with(inputs, true)
}

/// The notices for one file of `OUTPUTS`: the builds of its platforms alone.
pub fn notices_for(inputs: &Inputs, output: &Output) -> Result<(String, Assembly), Vec<String>> {
    let part = inputs.only(output.triples);
    if part.builds.is_empty() {
        return Err(vec![format!("{}: none of its platforms ({}) has a build", output.file, output.triples.join(", "))]);
    }
    notices_with(&part, output.checks_lists)
}

fn notices_with(inputs: &Inputs, check_lists: bool) -> Result<(String, Assembly), Vec<String>> {
    let assembly = assemble_with(inputs, check_lists);
    if !assembly.problems.is_empty() {
        return Err(assembly.problems);
    }
    Ok((render::render(&assembly, &inputs.builds), assembly))
}

fn problems_text(file: &str, problems: &[String]) -> String {
    format!("{} problem(s) with the third-party licences of {file}:\n  - {}", problems.len(), problems.join("\n  - "))
}

/// Prints what the notices hold, for each platform.
fn summary(assembly: &Assembly, inputs: &Inputs, file: &str) {
    for c in render::counts(assembly, &inputs.builds) {
        let programs: Vec<String> = c.programs.iter().map(|(p, n)| format!("{p} {n}")).collect();
        println!("licenses: {file}: {:<24} {} crates ({}), {} licences, {} texts", c.label, c.together, programs.join(", "), c.licences, c.texts);
    }
    println!(
        "licenses: {file}: {} crates in all, {} texts, {} of DiaCraft's own crates left out",
        assembly.entries.len(),
        assembly.texts.len(),
        assembly.own.len()
    );
    if !assembly.warnings.is_empty() {
        let names: Vec<&str> = assembly.warnings.iter().map(|w| w.split(':').next().unwrap_or(w)).collect();
        println!(
            "licenses: {file}: {} licence files are none of the licences this tool knows (a short COPYRIGHT or COPYING note, a notice of a licence): they are listed as they are ({})",
            names.len(),
            names.join("; ")
        );
    }
}

fn write(root: &Path) -> Result<(), String> {
    let inputs = gather::inputs(root)?;
    for output in OUTPUTS {
        let (text, assembly) = notices_for(&inputs, output).map_err(|p| problems_text(output.file, &p))?;
        let path = root.join(output.file);
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        summary(&assembly, &inputs.only(output.triples), output.file);
        println!("licenses: wrote {} ({} bytes)", output.file, text.len());
    }
    Ok(())
}

fn check(root: &Path) -> Result<(), String> {
    let inputs = gather::inputs(root)?;
    // Every file is looked at before the first failure is told, so that one run shows what is out of date.
    let mut failures = Vec::new();
    for output in OUTPUTS {
        let (text, assembly) = match notices_for(&inputs, output) {
            Ok(made) => made,
            Err(problems) => {
                failures.push(problems_text(output.file, &problems));
                continue;
            }
        };
        summary(&assembly, &inputs.only(output.triples), output.file);
        let path = root.join(output.file);
        let have = match std::fs::read_to_string(&path) {
            Ok(have) => have,
            Err(e) => {
                failures.push(format!("{}: {e} (make it with `cargo xtask licenses`)", path.display()));
                continue;
            }
        };
        if have != text {
            failures.push(format!(
                "{} is not what Cargo.lock and the crates' files say now ({} bytes kept, {} bytes made): run `cargo xtask licenses` and commit the file",
                output.file,
                have.len(),
                text.len()
            ));
            continue;
        }
        println!("licenses: {} is current", output.file);
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("\n")) }
}
