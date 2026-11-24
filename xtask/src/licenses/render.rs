//! The text of the notices file: what it is, how many crates each program is built from on each platform, the crates one by one, and every licence text once.
//! The text does not depend on the machine or the day it is made on (no path, no date, an order that does not follow the file system), so that a copy kept in the
//! repository can be compared with a fresh one.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::assemble::{Assembly, Build, Entry, Text};
use super::classify::standard_text;
use super::policy::RUST_HOLDERS;

const RULE: &str = "================================================================================";
/// The columns prose is broken at, so that the window (which breaks lines at 100) shows the lines as they are.
const PROSE: usize = 96;
const THIN: &str = "--------------------------------------------------------------------------------";

/// How many crates (leaving DiaCraft's own out) each program is built from on a platform, and how many distinct texts those crates carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counts {
    pub label: String,
    /// Per program: (name, crates).
    pub programs: Vec<(String, usize)>,
    /// The crates of all the programs together.
    pub together: usize,
    /// The distinct licence and notice texts of those crates.
    pub texts: usize,
    /// The distinct licences (SPDX ids of the choices) of those crates.
    pub licences: usize,
}

/// The counts for each platform.
pub fn counts(a: &Assembly, builds: &[Build]) -> Vec<Counts> {
    builds
        .iter()
        .enumerate()
        .map(|(at, build)| {
            let present = |e: &Entry| e.used_in.iter().any(|(_, places)| places.contains(&at));
            let entries: Vec<&Entry> = a.entries.iter().filter(|e| present(e)).collect();
            let texts: BTreeSet<usize> = entries.iter().flat_map(|e| e.texts.iter().copied()).collect();
            let licences: BTreeSet<&String> = entries.iter().flat_map(|e| e.chosen.iter()).collect();
            Counts {
                label: build.label.clone(),
                programs: build
                    .programs
                    .iter()
                    .map(|(program, _)| {
                        let n = a.entries.iter().filter(|e| e.used_in.iter().any(|(p, places)| p == program && places.contains(&at))).count();
                        (program.clone(), n)
                    })
                    .collect(),
                together: entries.len(),
                texts: texts.len(),
                licences: licences.len(),
            }
        })
        .collect()
}

/// The platforms where a crate is built in, as a list of names: the two macOS builds are said once when both have it.
fn platforms(places: &[usize], builds: &[Build]) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut mac: Vec<&str> = Vec::new();
    for at in places {
        if let Some(build) = builds.get(*at) {
            if build.label.starts_with("macOS") {
                mac.push(&build.label);
            } else {
                names.push(build.label.clone());
            }
        }
    }
    let macs = builds.iter().filter(|b| b.label.starts_with("macOS")).count();
    if mac.len() == macs && macs > 0 {
        names.push("macOS".to_owned());
    } else {
        names.extend(mac.iter().map(|m| (*m).to_owned()));
    }
    names.join(", ")
}

/// Where a crate is built in: one phrase when both programs have it on the same platforms, else one for each program.
fn used_phrase(e: &Entry, builds: &[Build]) -> String {
    let same = e.used_in.windows(2).all(|w| w.first().map(|a| &a.1) == w.get(1).map(|b| &b.1));
    if same && let Some((_, places)) = e.used_in.first() {
        let programs: Vec<&str> = e.used_in.iter().map(|(p, _)| p.as_str()).collect();
        return format!("{}: {}", programs.join(" and "), platforms(places, builds));
    }
    e.used_in.iter().map(|(program, places)| format!("{program}: {}", platforms(places, builds))).collect::<Vec<_>>().join("; ")
}

fn texts_phrase(e: &Entry) -> String {
    let numbers: Vec<String> = e.texts.iter().map(|t| (t + 1).to_string()).collect();
    format!("text {}", numbers.join(", "))
}

/// Writes the notices.
pub fn render(a: &Assembly, builds: &[Build]) -> String {
    let mut s = String::new();
    // Writing to a `String` does not fail.
    let _ = write_all(&mut s, a, builds);
    s
}

/// Whether the notices are the web build's (its builds are for `wasm32`): the words that name the programs and what they run on change with it.
fn is_web(builds: &[Build]) -> bool {
    !builds.is_empty() && builds.iter().all(|b| b.triple.starts_with("wasm32"))
}

fn write_all(s: &mut String, a: &Assembly, builds: &[Build]) -> std::fmt::Result {
    let web = is_web(builds);
    writeln!(s, "DiaCraft: third-party licences{}", if web { " (the web build)" } else { "" })?;
    writeln!(s, "{RULE}\n")?;
    if web {
        para(
            s,
            "DiaCraft is free software under the MIT License or the Apache License 2.0, at your option (LICENSE-MIT, LICENSE-APACHE, NOTICE). Its web build, `diacraft-web`, runs in a \
             browser as WebAssembly and is built from the Rust crates listed below, and from the Rust standard library. Each crate has its own licence, and the licence asks that its \
             notice and text go with the program. This file is that notice: it lists every crate the web build is built from, and gives each licence text once, with the crates that \
             carry it. The two fonts and the pictures DiaCraft carries are in NOTICE and ATTRIBUTION.md. The desktop programs have a list of their own, THIRD-PARTY-LICENSES.txt, which \
             every download carries.",
        )?;
    } else {
        para(
            s,
            "DiaCraft is free software under the MIT License or the Apache License 2.0, at your option (LICENSE-MIT, LICENSE-APACHE, NOTICE). Its programs, `diacraft` and \
             `diacraft-cli`, are built from the Rust crates listed below, and from the Rust standard library. Each crate has its own licence, and the licence asks that its notice \
             and text go with the programs. This file is that notice: it lists every crate the programs are built from, on each platform, and gives each licence text once, with \
             the crates that carry it. One crate, `libmimalloc-sys`, also compiles a C library into the programs; its entry says so. The two fonts and the pictures DiaCraft \
             carries are in NOTICE and ATTRIBUTION.md, which come in the download too.",
        )?;
    }
    para(
        s,
        "This file is made by `cargo xtask licenses` from Cargo.lock and the crates' own files, and `cargo xtask licenses --check` keeps it in step. Do not edit it by hand.",
    )?;
    writeln!(s, "{THIN}\n1. What is in it\n{THIN}\n")?;
    for c in counts(a, builds) {
        let programs: Vec<String> = c.programs.iter().map(|(p, n)| format!("{p} {n}")).collect();
        writeln!(s, "  {:<22} {} crates ({}), {} licences, {} texts", c.label, c.together, programs.join(", "), c.licences, c.texts)?;
    }
    writeln!(s)?;
    para(
        s,
        &format!(
            "The counts leave out DiaCraft's own {} crates, which are under DiaCraft's licence. A crate that offers a choice of licences (MIT OR Apache-2.0) is listed under \
             the one DiaCraft uses it under: MIT where it is offered, then Apache-2.0. {}",
            a.own.len(),
            if web {
                "The web build also uses what the browser provides (WebAssembly, the canvas, WebGL and WebGPU), which is not part of the program."
            } else {
                "The programs also use the computer's own libraries (its C library, the window system and the graphics drivers), which are not part of the download."
            }
        ),
    )?;
    writeln!(s, "{THIN}\n2. The Rust standard library\n{THIN}\n")?;
    para(
        s,
        "Every program is linked with the Rust standard library and its runtime (std, core, alloc and what they are built on). They are under MIT OR Apache-2.0, used here \
         under the MIT License. The Rust project keeps the notices of the code it includes in the COPYRIGHT file of its source: \
         https://github.com/rust-lang/rust/blob/master/COPYRIGHT",
    )?;
    writeln!(s, "{}\n", standard_text("MIT", RUST_HOLDERS).unwrap_or_default())?;
    writeln!(s, "{THIN}\n3. The crates\n{THIN}\n")?;
    for e in &a.entries {
        writeln!(s, "{} {}", e.name, e.version)?;
        let used = if e.chosen.len() == 1 && e.chosen.first().is_some_and(|c| *c == e.expression) {
            format!("licence {}", e.expression)
        } else {
            format!("licence {}, used under {}", e.expression, e.chosen.join(" and "))
        };
        writeln!(s, "    {used}; {}", texts_phrase(e))?;
        writeln!(s, "    {}", e.link)?;
        writeln!(s, "    built into {}", used_phrase(e, builds))?;
        for note in &e.notes {
            writeln!(s, "    {}", wrap(note, 92, "    "))?;
        }
    }
    writeln!(s, "\n{THIN}\n4. The texts\n{THIN}\n")?;
    writeln!(
        s,
        "Each text is given once. Where several crates carry the same words (the same licence with the same copyright line), they are listed together.\n\
         A crate that ships no text of its licence is listed with the licence's standard text and the authors its manifest names as the copyright holders.\n"
    )?;
    for (at, text) in a.texts.iter().enumerate() {
        write_text(s, at + 1, text)?;
    }
    writeln!(s, "{RULE}\nEnd of the third-party licences.")
}

/// One paragraph of prose, broken into lines of at most [`PROSE`] columns, and a blank line after it.
fn para(s: &mut String, text: &str) -> std::fmt::Result {
    let words: Vec<&str> = text.split_whitespace().collect();
    writeln!(s, "{}\n", wrap(&words.join(" "), PROSE, ""))
}

fn write_text(s: &mut String, number: usize, text: &Text) -> std::fmt::Result {
    writeln!(s, "{RULE}\nText {number}: {}\n{RULE}", text.title)?;
    let files: Vec<&str> = text.files.iter().map(String::as_str).collect();
    writeln!(s, "From: {}", files.join(", "))?;
    let users: Vec<&str> = text.users.iter().map(String::as_str).collect();
    writeln!(s, "Carried by: {}\n", wrap(&users.join(", "), 78, "    "))?;
    writeln!(s, "{}\n", text.body)
}

/// Breaks `text` (words separated by single spaces) into lines of at most `width` characters, the continuation lines indented by `indent`.
fn wrap(text: &str, width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line_len = 0usize;
    for word in text.split(' ') {
        let len = word.chars().count();
        if line_len > 0 && line_len + 1 + len > width {
            out.push('\n');
            out.push_str(indent);
            line_len = indent.len();
        } else if line_len > 0 {
            out.push(' ');
            line_len += 1;
        }
        out.push_str(word);
        line_len += len;
    }
    out
}
