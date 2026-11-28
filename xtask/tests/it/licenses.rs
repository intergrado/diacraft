//! `cargo xtask licenses`: the notices that go with the programs. The pure parts are tested on made-up crates in scratch folders; the
//! committed file is read as it is, and nothing here runs cargo.

use std::collections::BTreeSet;
use std::path::Path;

use xtask::licenses::assemble::{Allowed, Assembly, Build, Inputs, Package, assemble};
use xtask::licenses::classify::{class_of_id, classes, standard_text};
use xtask::licenses::files::{Place, discover, is_licence_name, is_notice_name, read_listed, read_text};
use xtask::licenses::gather::{allowed_from, packages_from, parse_tree};
use xtask::licenses::render::{counts, render};
use xtask::licenses::spdx::{self, Expr, choose, parse};

use crate::TempDir;

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn mit_text(holder: &str) -> String {
    format!(
        "MIT License\n\nCopyright (c) {holder}\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\n\
         of this software and associated documentation files (the \"Software\"), to deal\nin the Software without restriction.\n"
    )
}

fn apache_text() -> String {
    std::fs::read_to_string(xtask::root().join("LICENSE-APACHE")).unwrap()
}

fn allowed() -> Allowed {
    Allowed {
        global: ["MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "MPL-2.0", "OFL-1.1"].iter().map(|s| (*s).to_owned()).collect(),
        per_crate: Default::default(),
    }
}

fn package(dir: &Path, name: &str, license: Option<&str>) -> Package {
    Package {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        license: license.map(str::to_owned),
        authors: Vec::new(),
        repository: Some(format!("https://example.org/{name}")),
        source: Some(REGISTRY.to_owned()),
        dir: dir.join(name),
        own: false,
        native: false,
    }
}

/// One Linux build whose programs hold every package.
fn inputs(packages: Vec<Package>) -> Inputs {
    let all: BTreeSet<(String, String)> = packages.iter().map(|p| (p.name.clone(), p.version.clone())).collect();
    let build = Build {
        triple: "x86_64-unknown-linux-gnu".to_owned(),
        label: "Linux".to_owned(),
        programs: vec![("diacraft".to_owned(), all.clone()), ("diacraft-cli".to_owned(), all.clone())],
        together: all,
    };
    Inputs { packages, builds: vec![build], allowed: allowed() }
}

fn problems(a: &Assembly) -> String {
    a.problems.join("\n")
}

// ---------------------------------------------------------------------------------------------------------------------------------- SPDX

#[test]
fn an_expression_is_parsed_with_and_binding_tighter_than_or() {
    assert_eq!(parse("MIT").unwrap(), Expr::Id("MIT".into()));
    assert_eq!(parse("MIT OR Apache-2.0").unwrap(), Expr::Or(vec![Expr::Id("MIT".into()), Expr::Id("Apache-2.0".into())]));
    let both = parse("(MIT OR Apache-2.0) AND Unicode-3.0").unwrap();
    assert_eq!(both, Expr::And(vec![Expr::Or(vec![Expr::Id("MIT".into()), Expr::Id("Apache-2.0".into())]), Expr::Id("Unicode-3.0".into())]));
    let tight = parse("MIT OR ISC AND Zlib").unwrap();
    assert_eq!(tight, Expr::Or(vec![Expr::Id("MIT".into()), Expr::And(vec![Expr::Id("ISC".into()), Expr::Id("Zlib".into())])]));
}

#[test]
fn the_old_slash_means_or_and_an_exception_stays_with_its_licence() {
    assert_eq!(parse("MIT/Apache-2.0").unwrap(), parse("MIT OR Apache-2.0").unwrap());
    assert_eq!(parse("MIT / Apache-2.0").unwrap(), parse("MIT OR Apache-2.0").unwrap());
    assert_eq!(
        parse("Apache-2.0 WITH LLVM-exception OR MIT").unwrap(),
        Expr::Or(vec![Expr::Id("Apache-2.0 WITH LLVM-exception".into()), Expr::Id("MIT".into())])
    );
    assert_eq!(spdx::base_id("Apache-2.0 WITH LLVM-exception"), "Apache-2.0");
}

#[test]
fn a_broken_or_hostile_expression_is_an_error_and_not_a_panic() {
    for bad in ["", "MIT OR", "(MIT", "MIT)", "AND MIT", "MIT AND AND ISC", "Apache-2.0 WITH", "OR"] {
        assert!(parse(bad).is_err(), "`{bad}` parsed");
    }
    let deep = format!("{}MIT{}", "(".repeat(40), ")".repeat(40));
    let err = parse(&deep).unwrap_err();
    assert!(err.contains("nests parentheses"), "{err}");
    let long = "MIT OR ".repeat(2000) + "ISC";
    let err = parse(&long).unwrap_err();
    assert!(err.contains("bytes long"), "{err}");
}

fn permits(list: &'static [&'static str]) -> impl Fn(&str) -> bool {
    move |id| list.contains(&id)
}

#[test]
fn mit_is_chosen_wherever_it_is_offered_and_wherever_it_is_written() {
    let all = permits(&["MIT", "Apache-2.0", "Zlib", "BSD-3-Clause"]);
    for text in ["MIT OR Apache-2.0", "Apache-2.0 OR MIT", "Zlib OR Apache-2.0 OR MIT", "BSD-3-Clause OR Apache-2.0 OR MIT"] {
        assert_eq!(choose(&parse(text).unwrap(), &all), Ok(vec!["MIT".to_owned()]), "{text}");
    }
    assert_eq!(choose(&parse("BSD-3-Clause OR Apache-2.0").unwrap(), &all), Ok(vec!["Apache-2.0".to_owned()]));
    assert_eq!(choose(&parse("Zlib OR BSD-3-Clause").unwrap(), &all), Ok(vec!["BSD-3-Clause".to_owned()]));
}

#[test]
fn a_licence_the_list_refuses_is_never_chosen() {
    let list = permits(&["MIT", "Apache-2.0", "Unicode-3.0"]);
    assert_eq!(choose(&parse("Apache-2.0 OR GPL-2.0-only").unwrap(), &list), Ok(vec!["Apache-2.0".to_owned()]));
    assert_eq!(choose(&parse("Unlicense OR GPL-3.0").unwrap(), &list), Err(vec!["Unlicense".to_owned(), "GPL-3.0".to_owned()]));
    // An exception is a different licence to the list, as it is to cargo-deny.
    assert_eq!(choose(&parse("Apache-2.0 WITH LLVM-exception").unwrap(), &list), Err(vec!["Apache-2.0 WITH LLVM-exception".to_owned()]));
    assert_eq!(choose(&parse("Apache-2.0 WITH LLVM-exception OR MIT").unwrap(), &list), Ok(vec!["MIT".to_owned()]));
}

#[test]
fn an_and_takes_every_part_and_chooses_inside_its_parts() {
    let list = permits(&["MIT", "Apache-2.0", "Unicode-3.0"]);
    assert_eq!(choose(&parse("(MIT OR Apache-2.0) AND Unicode-3.0").unwrap(), &list), Ok(vec!["MIT".to_owned(), "Unicode-3.0".to_owned()]));
    // One part that is refused fails the whole.
    assert_eq!(choose(&parse("MIT AND GPL-2.0-only").unwrap(), &list), Err(vec!["GPL-2.0-only".to_owned()]));
    assert_eq!(choose(&parse("MIT AND MIT").unwrap(), &list), Ok(vec!["MIT".to_owned()]), "a licence named twice is used once");
}

// ------------------------------------------------------------------------------------------------------------------------------- classes

#[test]
fn the_permissive_licences_are_told_by_what_they_say() {
    assert_eq!(classes(&mit_text("2020 Alice")), ["MIT"]);
    assert_eq!(classes(&apache_text()), ["Apache-2.0"]);
    let bsd2 = "Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:\n\
                1. Redistributions of source code must retain the above copyright notice.\n2. Redistributions in binary form must reproduce it.";
    assert_eq!(classes(bsd2), ["BSD-2-Clause"]);
    let bsd3 = format!(
        "{bsd2}\n3. Neither the name of the holder nor the names of its contributors may be used to endorse or promote products derived from this software."
    );
    assert_eq!(classes(&bsd3), ["BSD-3-Clause"]);
    let isc = "Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.";
    assert_eq!(classes(isc), ["ISC"]);
    assert_eq!(classes(&isc.replace(", provided that the above copyright notice and this permission notice appear in all copies", "")), ["0BSD"]);
    assert_eq!(classes("Altered source versions must be plainly marked as such."), ["Zlib"]);
    assert_eq!(classes("Boost Software License - Version 1.0 - August 17th, 2003"), ["BSL-1.0"]);
    assert_eq!(classes("UNICODE LICENSE V3\n\nCOPYRIGHT AND PERMISSION NOTICE"), ["Unicode-3.0"]);
    assert_eq!(classes("CC0 1.0 Universal\n\nStatement of Purpose\nCreative Commons"), ["CC0-1.0"]);
    assert_eq!(classes("This is free and unencumbered software released into the public domain."), ["Unlicense"]);
    assert_eq!(classes("Mozilla Public License Version 2.0\n==================="), ["MPL-2.0"]);
    assert!(classes("see the file LICENSE-MIT").is_empty(), "a pointer to a licence is not one");
}

#[test]
fn a_text_with_comment_marks_in_front_of_its_lines_is_read_through_them() {
    let foxit = "// Redistribution and use in source and binary forms, with or without\n// modification, are permitted provided that the following conditions are\n\
                 // met:\n//    * Neither the name of Google Inc. nor the names of its\n// contributors may be used to endorse or promote products derived from\n";
    assert_eq!(classes(foxit), ["BSD-3-Clause"]);
}

/// The GNU General Public License ends by pointing to the Lesser one; the first run of the tool took `LICENSE-GPLv2` of `self_cell` for LGPL text and failed.
#[test]
fn the_gpl_that_names_the_lgpl_is_the_gpl_and_the_lgpl_is_the_lgpl() {
    let gpl = "                    GNU GENERAL PUBLIC LICENSE\n                       Version 2, June 1991\n\nTERMS AND CONDITIONS FOR COPYING\n\
               ...\nIf this is what you want to do, use the GNU Lesser General Public License instead of this License.";
    assert_eq!(classes(gpl), ["GPL-family"]);
    let lgpl = "                  GNU LESSER GENERAL PUBLIC LICENSE\n                       Version 2.1, February 1999\n\n Copyright (C) 1991, 1999 Free Software Foundation\n\
                Everyone is permitted to copy and distribute verbatim copies of the GNU General Public License";
    assert_eq!(classes(lgpl), ["LGPL-family"]);
    assert_eq!(classes("GNU AFFERO GENERAL PUBLIC LICENSE\nVersion 3"), ["AGPL-family"]);
    for (id, class) in [("GPL-2.0-only", "GPL-family"), ("LGPL-2.1-or-later", "LGPL-family"), ("AGPL-3.0", "AGPL-family"), ("MIT", "MIT")] {
        assert_eq!(class_of_id(id), class);
    }
    assert_eq!(class_of_id("Apache-2.0 WITH LLVM-exception"), "Apache-2.0");
}

/// The Open Font Licence grants "to any person obtaining a copy of the Font Software", which is not the MIT grant "of this software"; the first run titled the
/// fonts' licences MIT.
#[test]
fn the_open_font_licence_is_not_the_mit_licence() {
    let ofl = "SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n\nPERMISSION & CONDITIONS\nPermission is hereby granted, free of charge, to any person obtaining a copy\n\
               of the Font Software, to use, study, copy, merge, embed, modify, redistribute, and sell modified and unmodified copies of the Font Software.";
    assert_eq!(classes(ofl), ["OFL-1.1"]);
    let ubuntu =
        "UBUNTU FONT LICENCE Version 1.0\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of the Font Software.";
    assert_eq!(classes(ubuntu), ["Ubuntu-font-1.0"]);
}

#[test]
fn a_standard_text_names_the_holders_and_only_the_short_licences_have_one() {
    let mit = standard_text("MIT", "Alice, Bob").unwrap();
    assert!(mit.contains("Copyright (c) Alice, Bob") && !mit.contains('@'), "{mit}");
    assert_eq!(classes(&mit), ["MIT"]);
    for (class, expected) in
        [("BSD-2-Clause", "BSD-2-Clause"), ("BSD-3-Clause", "BSD-3-Clause"), ("ISC", "ISC"), ("Zlib", "Zlib"), ("BSL-1.0", "BSL-1.0")]
    {
        let text = standard_text(class, "Alice").unwrap();
        assert_eq!(classes(&text), [expected], "{class}");
        assert!(text.contains("Alice"), "{class}");
    }
    assert_eq!(classes(&standard_text("Apache-2.0", "x").unwrap()), ["Apache-2.0"]);
    for class in ["MPL-2.0", "OFL-1.1", "Unicode-3.0", "GPL-family", "nothing"] {
        assert!(standard_text(class, "x").is_none(), "{class}");
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------- files

#[test]
fn a_licence_file_is_told_by_its_name_and_a_source_file_that_has_one_is_not() {
    for yes in [
        "LICENSE",
        "LICENSE-MIT",
        "LICENSE_APACHE",
        "license.txt",
        "LICENSE.md",
        "COPYING",
        "COPYING.LIB",
        "NOTICE",
        "UNLICENSE",
        "LICENCE",
        "LICENSE_FOXIT",
        "COPYRIGHT",
    ] {
        assert!(is_licence_name(yes), "{yes}");
    }
    for no in ["copying.rs", "license.rs", "licenses.json", "README.md", "Cargo.toml", "licensing.md", "notices.md", "lib.rs", "LICENSE-MIT.c"] {
        assert!(!is_licence_name(no), "{no}");
    }
    assert!(is_notice_name("NOTICE") && is_notice_name("notice.txt") && !is_notice_name("LICENSE"));
}

#[test]
fn a_crates_own_licences_its_licences_folder_and_the_nested_ones_are_found_and_the_tests_are_not() {
    let dir = TempDir::new("files-find");
    let krate = dir.path().join("c");
    for (rel, text) in [
        ("LICENSE-MIT", "own mit"),
        ("NOTICE", "own notice"),
        ("licenses/MIT.txt", "licences folder"),
        ("c_src/lib/LICENSE", "nested c"),
        ("src/copying.rs", "not a licence"),
        ("tests/data/LICENSE", "in the tests"),
        ("docs/LICENSE", "in the docs"),
        ("target/LICENSE", "in a build folder"),
        ("README.md", "not a licence"),
    ] {
        dir.write(&format!("c/{rel}"), text);
    }
    let found = discover(&krate, None).unwrap();
    let got: Vec<(&str, Place)> = found.iter().map(|f| (f.rel.as_str(), f.place)).collect();
    assert_eq!(got, [("LICENSE-MIT", Place::Own), ("NOTICE", Place::Own), ("licenses/MIT.txt", Place::Own), ("c_src/lib/LICENSE", Place::Nested)]);
}

#[test]
fn a_crate_in_a_checkout_gets_the_files_of_the_folders_above_it_up_to_the_checkout() {
    let dir = TempDir::new("files-above");
    dir.write("checkouts/repo-abc/1234567/LICENSE-MIT", "root mit");
    dir.write("checkouts/repo-abc/1234567/NOTICE", "root notice");
    dir.write("checkouts/repo-abc/1234567/crates/x/LICENSE-APACHE", "own apache");
    dir.write("checkouts/LICENSE", "above the checkout: not the repository's");
    let root = dir.path().join("checkouts/repo-abc/1234567");
    let found = discover(&root.join("crates/x"), Some(&root)).unwrap();
    let got: Vec<(&str, Place)> = found.iter().map(|f| (f.rel.as_str(), f.place)).collect();
    assert_eq!(got, [("LICENSE-APACHE", Place::Own), ("LICENSE-MIT", Place::Above), ("NOTICE", Place::Above)]);
}

#[test]
fn a_file_is_read_with_unix_line_ends_whatever_its_bytes_and_a_big_one_is_refused() {
    let dir = TempDir::new("files-read");
    dir.write_bytes("crlf", b"line one\r\nline two\rline three\n");
    assert_eq!(read_text(&dir.path().join("crlf")).unwrap(), "line one\nline two\nline three\n");
    dir.write_bytes("bom", b"\xEF\xBB\xBFMIT");
    assert_eq!(read_text(&dir.path().join("bom")).unwrap(), "MIT");
    // Latin-1: the byte 0xA9 is the copyright sign.
    dir.write_bytes("latin1", b"Copyright \xA9 2001");
    assert_eq!(read_text(&dir.path().join("latin1")).unwrap(), "Copyright \u{a9} 2001");
    dir.write_bytes("big", &vec![b'a'; 1024 * 1024 + 1]);
    let err = read_text(&dir.path().join("big")).unwrap_err();
    assert!(err.contains("not read"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_link_named_like_a_licence_is_not_followed() {
    let dir = TempDir::new("files-link");
    dir.write("secret/passwords", "do not read me");
    dir.write("c/LICENSE-MIT", "mit");
    std::os::unix::fs::symlink(dir.path().join("secret/passwords"), dir.path().join("c/LICENSE")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("secret"), dir.path().join("c/licenses")).unwrap();
    let found = discover(&dir.path().join("c"), None).unwrap();
    assert_eq!(found.iter().map(|f| f.rel.as_str()).collect::<Vec<_>>(), ["LICENSE-MIT"]);
}

#[test]
fn a_listed_file_that_is_missing_or_leaves_the_crate_is_an_error() {
    let dir = TempDir::new("files-listed");
    dir.write("c/fonts/OFL.txt", "ofl");
    let found = read_listed(&dir.path().join("c"), &["fonts/OFL.txt"]).unwrap();
    assert_eq!((found[0].rel.as_str(), found[0].place), ("fonts/OFL.txt", Place::Nested));
    let err = read_listed(&dir.path().join("c"), &["fonts/gone.txt"]).unwrap_err();
    assert!(err.contains("fonts/gone.txt"), "{err}");
    let err = read_listed(&dir.path().join("c"), &["../c/fonts/OFL.txt"]).unwrap_err();
    assert!(err.contains("not a path inside the crate"), "{err}");
}

// -------------------------------------------------------------------------------------------------------------------------------- assemble

/// The files of a made-up crate, each as (path in the crate, text).
type Files<'a> = &'a [(&'a str, String)];

/// A made-up crate: its name, its licence expression, its files.
type Spec<'a> = (&'a str, Option<&'a str>, Files<'a>);

/// A registry of crates in a scratch folder: each crate is a folder holding the files given.
fn registry(tag: &str, crates: &[Spec<'_>]) -> (TempDir, Inputs) {
    let dir = TempDir::new(tag);
    let mut packages = Vec::new();
    for (name, license, files) in crates {
        std::fs::create_dir_all(dir.path().join(name)).unwrap();
        for (rel, text) in *files {
            dir.write(&format!("{name}/{rel}"), text);
        }
        packages.push(package(dir.path(), name, *license));
    }
    let inputs = inputs(packages);
    (dir, inputs)
}

fn text_of<'a>(a: &'a Assembly, user: &str) -> Vec<&'a str> {
    a.texts.iter().filter(|t| t.users.iter().any(|u| u.starts_with(user))).map(|t| t.body.as_str()).collect()
}

#[test]
fn a_dual_licensed_crate_is_listed_with_the_mit_text_and_not_the_apache_one() {
    let (_dir, inputs) =
        registry("asm-dual", &[("both", Some("MIT OR Apache-2.0"), &[("LICENSE-MIT", mit_text("2020 Alice")), ("LICENSE-APACHE", apache_text())])]);
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    assert_eq!(a.entries.len(), 1);
    assert_eq!(a.entries[0].chosen, ["MIT"]);
    assert_eq!(text_of(&a, "both").len(), 1, "one text");
    assert!(text_of(&a, "both")[0].contains("Copyright (c) 2020 Alice"));
}

#[test]
fn two_crates_with_the_same_words_share_one_text_and_a_different_copyright_line_is_another() {
    let same = mit_text("2020 Alice");
    let (_dir, inputs) = registry(
        "asm-dedupe",
        &[
            ("a", Some("MIT"), &[("LICENSE", same.clone())]),
            ("b", Some("MIT"), &[("LICENSE-MIT", same.replace('\n', "\r\n"))]),
            ("c", Some("MIT"), &[("LICENSE", mit_text("2021 Bob"))]),
        ],
    );
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    assert_eq!(a.texts.len(), 2);
    let shared = a.texts.iter().find(|t| t.users.len() == 2).unwrap();
    assert_eq!(shared.users.iter().cloned().collect::<Vec<_>>(), ["a 1.0.0", "b 1.0.0"]);
    assert_eq!(shared.title, "MIT License");
}

#[test]
fn a_crate_that_ships_no_text_gets_the_standard_one_with_its_authors_and_no_addresses() {
    let (_dir, mut inputs) = registry("asm-standard", &[("bare", Some("MIT OR Apache-2.0"), &[("README.md", "nothing".to_owned())])]);
    inputs.packages[0].authors = vec!["Alice Smith <alice@example.org>".to_owned(), "The Bare contributors".to_owned()];
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    let text = text_of(&a, "bare")[0];
    assert!(text.contains("Copyright (c) Alice Smith, The Bare contributors"), "{text}");
    assert!(!text.contains("alice@example.org"), "an address is not a copyright holder");
    assert!(a.texts[0].files.contains("standard text"));
    // A crate with no author at all is named after itself.
    let (_dir, inputs) = registry("asm-standard2", &[("anon", Some("MIT"), &[])]);
    assert!(text_of(&assemble(&inputs), "anon")[0].contains("Copyright (c) the authors of anon"));
}

#[test]
fn a_licence_with_no_file_and_no_standard_text_fails_the_check_naming_it() {
    let (_dir, inputs) = registry("asm-nofile", &[("fonty", Some("OFL-1.1"), &[("README.md", String::new())])]);
    let a = assemble(&inputs);
    assert_eq!(a.problems.len(), 1, "{}", problems(&a));
    assert!(
        a.problems[0].contains("fonty 1.0.0") && a.problems[0].contains("OFL-1.1") && a.problems[0].contains("no standard text"),
        "{}",
        a.problems[0]
    );
}

#[test]
fn a_licence_that_deny_toml_does_not_allow_fails_the_check_and_a_missing_one_does_too() {
    let (_dir, inputs) = registry(
        "asm-refused",
        &[
            ("copyleft", Some("GPL-3.0-only"), &[]),
            ("either", Some("GPL-2.0-only OR LGPL-2.1-only"), &[]),
            ("blank", None, &[]),
            ("garbled", Some("MIT OR"), &[]),
        ],
    );
    let a = assemble(&inputs);
    let all = problems(&a);
    assert!(all.contains("copyleft 1.0.0") && all.contains("GPL-3.0-only"), "{all}");
    assert!(all.contains("either 1.0.0") && all.contains("GPL-2.0-only, LGPL-2.1-only"), "{all}");
    assert!(all.contains("blank 1.0.0: its manifest has no licence expression"), "{all}");
    assert!(all.contains("garbled 1.0.0"), "{all}");
    assert!(a.entries.is_empty(), "a crate with a problem is not listed");
}

#[test]
fn a_per_crate_exception_in_deny_toml_lets_that_crate_and_no_other_use_its_licence() {
    let (_dir, mut inputs) = registry(
        "asm-exception",
        &[("certs", Some("CDLA-Permissive-2.0"), &[("LICENSE", "CDLA text".to_owned())]), ("other", Some("CDLA-Permissive-2.0"), &[])],
    );
    inputs.allowed.per_crate.entry("certs".to_owned()).or_default().insert("CDLA-Permissive-2.0".to_owned());
    let a = assemble(&inputs);
    assert!(a.entries.iter().any(|e| e.name == "certs"));
    assert!(problems(&a).contains("other 1.0.0") && !problems(&a).contains("certs 1.0.0"), "{}", problems(&a));
}

#[test]
fn a_gnu_text_that_is_the_other_half_of_an_or_is_left_out_and_one_that_is_not_offered_fails() {
    let gpl = "GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991".to_owned();
    let (_dir, inputs) = registry(
        "asm-gnu",
        &[
            ("offered", Some("Apache-2.0 OR GPL-2.0-only"), &[("LICENSE-APACHE", apache_text()), ("LICENSE-GPLv2", gpl.clone())]),
            ("hidden", Some("MIT"), &[("LICENSE-MIT", mit_text("2020 Alice")), ("vendor/LICENSE", gpl)]),
        ],
    );
    let a = assemble(&inputs);
    assert!(text_of(&a, "offered").iter().all(|t| !t.contains("GNU GENERAL")), "the GPL text of an unused alternative is not listed");
    assert_eq!(a.problems.len(), 1, "{}", problems(&a));
    assert!(a.problems[0].starts_with("hidden 1.0.0: vendor/LICENSE is GNU General Public License text"), "{}", a.problems[0]);
}

#[test]
fn a_nested_licence_is_listed_and_a_reviewed_one_outside_the_list_is_listed_with_a_note() {
    let lgpl = "GNU LESSER GENERAL PUBLIC LICENSE\nVersion 2.1, February 1999".to_owned();
    let (_dir, inputs) = registry(
        "asm-nested",
        &[
            (
                "wayland-protocols-plasma",
                Some("MIT"),
                &[("LICENSE.txt", mit_text("2015 Elinor")), ("plasma-wayland-protocols/COPYING.LIB", lgpl.clone())],
            ),
            (
                "libmimalloc-sys",
                Some("MIT"),
                &[("LICENSE.txt", mit_text("2019 Octavian")), ("c_src/mimalloc/v2/LICENSE", mit_text("2018 the allocator's authors"))],
            ),
        ],
    );
    let mut inputs = inputs;
    inputs.packages[1].native = true;
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    let plasma = a.entries.iter().find(|e| e.name == "wayland-protocols-plasma").unwrap();
    assert_eq!(plasma.texts.len(), 2);
    assert!(plasma.notes.iter().any(|n| n.contains("COPYING.LIB") && n.contains("Lesser General Public License")), "{:?}", plasma.notes);
    let mimalloc = a.entries.iter().find(|e| e.name == "libmimalloc-sys").unwrap();
    assert_eq!(mimalloc.texts.len(), 2, "the crate's and the C library's");
    assert!(mimalloc.notes.iter().any(|n| n.starts_with("C code:") && n.contains("mimalloc")), "{:?}", mimalloc.notes);
    assert!(text_of(&a, "libmimalloc-sys").iter().any(|t| t.contains("the allocator's authors")));
}

#[test]
fn a_crate_that_compiles_c_code_the_policy_does_not_name_fails_the_check() {
    let (_dir, mut inputs) = registry("asm-native", &[("shiny-sys", Some("MIT"), &[("LICENSE", mit_text("2020 Alice"))])]);
    inputs.packages[0].native = true;
    let a = assemble(&inputs);
    assert_eq!(a.problems.len(), 1, "{}", problems(&a));
    assert!(a.problems[0].contains("shiny-sys") && a.problems[0].contains("NATIVE_CODE"), "{}", a.problems[0]);
}

#[test]
fn a_notice_is_kept_for_an_apache_crate_and_for_a_notice_of_the_crate_but_not_a_repositorys_for_an_mit_one() {
    let dir = TempDir::new("asm-notice");
    let checkout = "checkouts/repo-abc/1234567";
    dir.write(&format!("{checkout}/NOTICE"), "the repository's notice for all its crates");
    dir.write(&format!("{checkout}/LICENSE-MIT"), &mit_text("2026 the repository"));
    dir.write(&format!("{checkout}/crates/mit-one/Cargo.toml"), "");
    dir.write(&format!("{checkout}/crates/apache-one/LICENSE"), &apache_text());
    dir.write("plain/LICENSE-MIT", &mit_text("2018 Yevhenii"));
    dir.write("plain/NOTICE", "adapted from another crate, under the MIT licence");
    let git = |name: &str, license: &str| Package {
        source: Some("git+https://example.org/repo?rev=abc#abc".to_owned()),
        dir: dir.path().join(checkout).join("crates").join(name),
        ..package(dir.path(), name, Some(license))
    };
    let mut plain = package(dir.path(), "plain", Some("MIT OR Apache-2.0"));
    plain.dir = dir.path().join("plain");
    let a = assemble(&inputs(vec![git("mit-one", "MIT OR Apache-2.0"), git("apache-one", "Apache-2.0"), plain]));
    assert!(a.problems.is_empty(), "{}", problems(&a));
    assert!(text_of(&a, "mit-one").iter().all(|t| !t.contains("repository's notice")), "the repository's NOTICE is not part of the MIT terms");
    assert!(text_of(&a, "mit-one").iter().any(|t| t.contains("2026 the repository")), "its LICENSE-MIT above it is");
    assert!(text_of(&a, "apache-one").iter().any(|t| t.contains("repository's notice")), "Apache 2.0 section 4(d)");
    assert!(text_of(&a, "plain").iter().any(|t| t.contains("adapted from another crate")), "a crate's own NOTICE is kept");
}

#[test]
fn an_mpl_crate_says_where_its_source_is() {
    let (_dir, inputs) = registry("asm-mpl", &[("mozzy", Some("MPL-2.0"), &[("LICENSE", "Mozilla Public License Version 2.0".to_owned())])]);
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    let note = &a.entries[0].notes[0];
    assert!(
        note.starts_with("Mozilla Public License 2.0:") && note.contains("unmodified") && note.contains("https://crates.io/crates/mozzy/1.0.0"),
        "{note}"
    );
}

#[test]
fn a_crate_of_the_workspace_has_no_entry_and_one_that_cargo_tree_lists_and_metadata_lacks_is_a_problem() {
    let (_dir, mut inputs) =
        registry("asm-own", &[("diacraft-engine", Some("MIT OR Apache-2.0"), &[]), ("dep", Some("MIT"), &[("LICENSE", mit_text("2020 Alice"))])]);
    inputs.packages[0].own = true;
    inputs.builds[0].together.insert(("ghost".to_owned(), "9.9.9".to_owned()));
    let a = assemble(&inputs);
    assert_eq!(a.own, ["diacraft-engine"]);
    assert_eq!(a.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["dep"]);
    assert_eq!(a.problems, ["ghost 9.9.9: `cargo tree` lists it and `cargo metadata` does not"]);
}

#[test]
fn a_crate_that_is_in_no_build_is_not_listed() {
    let (_dir, mut inputs) = registry("asm-unused", &[("used", Some("MIT"), &[("LICENSE", mit_text("a"))]), ("unused", Some("GPL-3.0-only"), &[])]);
    inputs.builds[0].together.remove(&("unused".to_owned(), "1.0.0".to_owned()));
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "a crate no program is built from is not looked at: {}", problems(&a));
    assert_eq!(a.entries.len(), 1);
}

#[test]
fn the_notices_do_not_depend_on_the_order_the_crates_come_in() {
    type Owned<'a> = (&'a str, Option<&'a str>, Vec<(&'a str, String)>);
    let crates: Vec<Owned<'_>> = vec![
        ("zeta", Some("MIT"), vec![("LICENSE", mit_text("2020 Zed"))]),
        ("alpha", Some("MIT OR Apache-2.0"), vec![("LICENSE-APACHE", apache_text())]),
        ("mid", Some("MIT"), vec![("LICENSE", mit_text("2020 Mid"))]),
    ];
    let make = |order: &[usize]| {
        let list: Vec<Spec<'_>> = order.iter().map(|i| (crates[*i].0, crates[*i].1, crates[*i].2.as_slice())).collect();
        let (_dir, inputs) = registry("asm-order", &list);
        let a = assemble(&inputs);
        render(&a, &inputs.builds)
    };
    assert_eq!(make(&[0, 1, 2]), make(&[2, 0, 1]));
    assert_eq!(make(&[0, 1, 2]), make(&[1, 2, 0]));
}

// ---------------------------------------------------------------------------------------------------------------------------------- render

fn two_platform_inputs() -> (TempDir, Inputs) {
    let (dir, mut inputs) = registry(
        "render",
        &[
            ("everywhere", Some("MIT"), &[("LICENSE", mit_text("2020 Alice"))]),
            ("linux-only", Some("MIT"), &[("LICENSE", mit_text("2020 Bob"))]),
            ("cli-free", Some("Apache-2.0"), &[("LICENSE", apache_text())]),
        ],
    );
    let key = |n: &str| (n.to_owned(), "1.0.0".to_owned());
    let set = |names: &[&str]| names.iter().map(|n| key(n)).collect::<BTreeSet<_>>();
    inputs.builds = vec![
        Build {
            triple: "x86_64-unknown-linux-gnu".into(),
            label: "Linux".into(),
            programs: vec![
                ("diacraft".into(), set(&["everywhere", "linux-only", "cli-free"])),
                ("diacraft-cli".into(), set(&["everywhere", "linux-only"])),
            ],
            together: set(&["everywhere", "linux-only", "cli-free"]),
        },
        Build {
            triple: "aarch64-apple-darwin".into(),
            label: "macOS (Apple silicon)".into(),
            programs: vec![("diacraft".into(), set(&["everywhere", "cli-free"])), ("diacraft-cli".into(), set(&["everywhere"]))],
            together: set(&["everywhere", "cli-free"]),
        },
        Build {
            triple: "x86_64-apple-darwin".into(),
            label: "macOS (Intel)".into(),
            programs: vec![("diacraft".into(), set(&["everywhere", "cli-free"])), ("diacraft-cli".into(), set(&["everywhere"]))],
            together: set(&["everywhere", "cli-free"]),
        },
    ];
    (dir, inputs)
}

#[test]
fn the_counts_say_how_many_crates_each_program_is_built_from_on_each_platform() {
    let (_dir, inputs) = two_platform_inputs();
    let a = assemble(&inputs);
    assert!(a.problems.is_empty(), "{}", problems(&a));
    let c = counts(&a, &inputs.builds);
    assert_eq!(c.len(), 3);
    assert_eq!((c[0].together, c[0].programs.clone()), (3, vec![("diacraft".to_owned(), 3), ("diacraft-cli".to_owned(), 2)]));
    assert_eq!((c[1].together, c[1].programs.clone()), (2, vec![("diacraft".to_owned(), 2), ("diacraft-cli".to_owned(), 1)]));
    assert_eq!((c[0].licences, c[0].texts), (2, 3), "MIT and Apache-2.0; three texts");
    assert_eq!((c[1].licences, c[1].texts), (2, 2));
}

#[test]
fn the_file_lists_each_crate_with_where_it_is_built_in_and_the_two_macs_are_one_name() {
    let (_dir, inputs) = two_platform_inputs();
    let a = assemble(&inputs);
    let text = render(&a, &inputs.builds);
    assert!(text.contains("everywhere 1.0.0\n    licence MIT; text "), "{text}");
    assert!(text.contains("built into diacraft and diacraft-cli: Linux, macOS\n"), "{text}");
    assert!(text.contains("linux-only 1.0.0") && text.contains("built into diacraft and diacraft-cli: Linux\n"), "{text}");
    assert!(text.contains("built into diacraft: Linux, macOS\n"), "a crate only the app has: {text}");
    assert!(text.contains(&format!("{:<22} 3 crates (diacraft 3, diacraft-cli 2)", "Linux")), "{text}");
    assert!(text.contains("The Rust standard library") && text.contains("Copyright (c) The Rust Project Contributors"), "{text}");
    assert!(text.ends_with("End of the third-party licences.\n"), "{text}");
    assert_eq!(render(&a, &inputs.builds), text, "the same input gives the same file");
}

#[test]
fn a_crate_built_into_a_different_set_of_platforms_in_each_program_says_so_for_each() {
    let (_dir, mut inputs) = two_platform_inputs();
    // `cli-free` becomes a crate of the command line on Linux and of the app on both Macs.
    inputs.builds[0].programs[1].1.insert(("cli-free".to_owned(), "1.0.0".to_owned()));
    inputs.builds[0].programs[0].1.remove(&("cli-free".to_owned(), "1.0.0".to_owned()));
    let a = assemble(&inputs);
    let text = render(&a, &inputs.builds);
    assert!(text.contains("built into diacraft: macOS; diacraft-cli: Linux\n"), "{text}");
}

// ----------------------------------------------------------------------------------------------------------------------------------- gather

#[test]
fn the_crates_of_cargo_tree_are_read_from_its_lines_with_their_marks() {
    let text = "diacraft v0.1.0 (/home/me/diacraft/apps/diacraft)\nserde v1.0.228\nserde_core v1.0.228 (*)\n\
                egui v0.37.0 (https://github.com/storytold/egui.git?rev=310f43c5#310f43c5)\n\nnot a crate line\nx\nv1.0.0 y\n";
    let got: Vec<(String, String)> = parse_tree(text).into_iter().collect();
    assert_eq!(
        got,
        [
            ("diacraft".to_owned(), "0.1.0".to_owned()),
            ("egui".to_owned(), "0.37.0".to_owned()),
            ("serde".to_owned(), "1.0.228".to_owned()),
            ("serde_core".to_owned(), "1.0.228".to_owned())
        ]
    );
}

#[test]
fn the_packages_of_cargo_metadata_are_read_with_their_licences_and_a_build_dependency_on_cc_marks_native_code() {
    let root = Path::new("/work/diacraft");
    let json = serde_json::json!({
        "packages": [
            {"id": "own 0.1.0 (path+file:///work/diacraft/crates/own)", "name": "own", "version": "0.1.0", "license": "MIT OR Apache-2.0", "authors": [],
             "repository": null, "source": null, "manifest_path": "/work/diacraft/crates/own/Cargo.toml"},
            {"id": "alloc-sys 0.1.0 (registry)", "name": "alloc-sys", "version": "0.1.0", "license": "MIT", "authors": ["A <a@x.org>", "B"],
             "repository": "https://example.org/alloc", "source": "registry+https://github.com/rust-lang/crates.io-index",
             "manifest_path": "/home/me/.cargo/registry/src/index/alloc-sys-0.1.0/Cargo.toml"},
            {"id": "cc 1.0.0 (registry)", "name": "cc", "version": "1.0.0", "license": "MIT OR Apache-2.0", "authors": [], "repository": null,
             "source": "registry+https://github.com/rust-lang/crates.io-index", "manifest_path": "/home/me/.cargo/registry/src/index/cc-1.0.0/Cargo.toml"},
            {"id": "outside 0.1.0 (path+file:///elsewhere)", "name": "outside", "version": "0.1.0", "license": null, "authors": [], "repository": null,
             "source": null, "manifest_path": "/elsewhere/outside/Cargo.toml"}
        ],
        "resolve": {"nodes": [
            {"id": "alloc-sys 0.1.0 (registry)", "deps": [{"pkg": "cc 1.0.0 (registry)", "dep_kinds": [{"kind": "build", "target": null}]}]},
            {"id": "own 0.1.0 (path+file:///work/diacraft/crates/own)", "deps": [{"pkg": "cc 1.0.0 (registry)", "dep_kinds": [{"kind": "dev", "target": null}]}]}
        ]}
    });
    let packages = packages_from(json.to_string().as_bytes(), root).unwrap();
    let get = |name: &str| packages.iter().find(|p| p.name == name).unwrap();
    assert!(get("own").own && !get("own").native, "a dev dependency on cc is not C code in a program");
    assert!(!get("alloc-sys").own && get("alloc-sys").native);
    assert_eq!(get("alloc-sys").authors, ["A <a@x.org>", "B"]);
    assert_eq!(get("alloc-sys").repository.as_deref(), Some("https://example.org/alloc"));
    assert_eq!(get("alloc-sys").dir, Path::new("/home/me/.cargo/registry/src/index/alloc-sys-0.1.0"));
    assert!(!get("outside").own, "a crate on a path outside the workspace is not DiaCraft's");
    assert_eq!(get("outside").license, None);
    assert!(packages_from(b"{}", root).is_err() && packages_from(b"not json", root).is_err());
}

#[test]
fn deny_toml_is_read_for_its_allowed_licences_and_its_per_crate_exceptions() {
    let allowed = allowed_from(
        "[licenses]\nallow = [\"MIT\", \"Apache-2.0\"]\n\n[[licenses.exceptions]]\ncrate = \"certs\"\nallow = [\"CDLA-Permissive-2.0\"]\n",
    )
    .unwrap();
    assert!(allowed.permits("any", "MIT") && !allowed.permits("any", "CDLA-Permissive-2.0"));
    assert!(allowed.permits("certs", "CDLA-Permissive-2.0") && !allowed.permits("certs", "GPL-3.0-only"));
    for bad in [
        "",
        "[licenses]\n",
        "[licenses]\nallow = \"MIT\"\n",
        "[licenses]\nallow = [1]\n",
        "[licenses]\nallow = []\n[[licenses.exceptions]]\nallow = []\n",
        "not toml [",
    ] {
        assert!(allowed_from(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn this_repositorys_deny_toml_reads_and_allows_what_the_policy_says() {
    let text = std::fs::read_to_string(xtask::root().join("deny.toml")).unwrap();
    let allowed = allowed_from(&text).unwrap();
    for id in ["MIT", "Apache-2.0", "BSD-3-Clause", "Unicode-3.0", "OFL-1.1", "Ubuntu-font-1.0"] {
        assert!(allowed.permits("anything", id), "{id}");
    }
    for id in ["GPL-3.0-only", "LGPL-2.1-or-later", "AGPL-3.0-only", "CDLA-Permissive-2.0"] {
        assert!(!allowed.permits("anything", id), "{id}");
    }
}

// --------------------------------------------------------------------------------------------------------------------- the committed file

fn committed() -> String {
    std::fs::read_to_string(xtask::root().join("THIRD-PARTY-LICENSES.txt")).unwrap()
}

#[test]
fn the_committed_notices_name_the_programs_the_platforms_and_the_c_library() {
    let text = committed();
    assert!(text.starts_with("DiaCraft: third-party licences\n"), "{}", &text[..80]);
    for word in ["diacraft-cli", "Linux", "Windows", "macOS (Apple silicon)", "macOS (Intel)", "libmimalloc-sys", "The Rust standard library"] {
        assert!(text.contains(word), "the notices do not say `{word}`");
    }
    // The mimalloc C library's own licence, and the licences of the two fonts the window carries.
    assert!(text.contains("mimalloc"), "the C library is named");
    assert!(text.contains("Hack-Regular.txt") && text.contains("UFL.txt"), "the fonts' licences are in");
    assert!(text.contains("Text 1: "), "the texts are numbered");
    assert!(text.len() > 100_000 && text.len() < 4 * 1024 * 1024, "{} bytes", text.len());
    assert!(text.ends_with("End of the third-party licences.\n"));
}

#[test]
fn the_committed_notices_hold_no_crate_the_programs_are_not_built_from() {
    let text = committed();
    // Crates of the workspace-wide resolution that the two programs do not compile: the OpenGL backend and the PDF reader.
    // `arboard` is not here: the desktop app builds eframe with its `clipboard` feature and compiles it.
    for name in ["glutin 0.32", "egui_glow", "hayro-interpret"] {
        assert!(!text.contains(&format!("\n{name}")), "`{name}` is listed, and `cargo build -p diacraft -p diacraft-cli` does not compile it");
    }
    // The TLS stack and its C libraries are a build dependency of the Windows print library's build script, and in no program.
    for name in ["ring ", "aws-lc-sys ", "aws-lc-rs ", "rustls "] {
        assert!(!text.contains(&format!("\n{name}")), "`{name}` is listed, and no program holds it");
    }
}

#[test]
fn every_crate_in_the_committed_notices_points_at_a_text_that_exists() {
    let text = committed();
    let texts: usize = text
        .lines()
        .filter(|l| l.strip_prefix("Text ").and_then(|rest| rest.split_once(": ")).is_some_and(|(n, _)| n.parse::<usize>().is_ok()))
        .count();
    assert!(texts > 50, "{texts}");
    let mut crates = 0;
    for line in text.lines().filter(|l| l.starts_with("    licence ")) {
        crates += 1;
        let (_, numbers) = line.rsplit_once("text ").unwrap_or_else(|| panic!("{line}"));
        for n in numbers.split(", ") {
            let n: usize = n.trim().parse().unwrap_or_else(|_| panic!("{line}"));
            assert!((1..=texts).contains(&n), "{line}: text {n} of {texts}");
        }
    }
    assert!(crates > 200, "{crates}");
}

// ------------------------------------------------------------------------------------------------------------------ the web build

/// A desktop build and a web build over three made-up crates: `shared` is in both, `wayland-ish` in the desktop one only, `wasm-glue` in the web one only.
fn desktop_and_web_inputs() -> (TempDir, Inputs) {
    let (dir, mut inputs) = registry(
        "web-notices",
        &[
            ("shared", Some("MIT"), &[("LICENSE", mit_text("2020 Alice"))]),
            ("wayland-ish", Some("MIT"), &[("LICENSE", mit_text("2020 Bob"))]),
            ("wasm-glue", Some("MIT"), &[("LICENSE", mit_text("2020 Carol"))]),
        ],
    );
    let set = |names: &[&str]| names.iter().map(|n| ((*n).to_owned(), "1.0.0".to_owned())).collect::<BTreeSet<_>>();
    inputs.builds = vec![
        Build {
            triple: "x86_64-unknown-linux-gnu".into(),
            label: "Linux".into(),
            programs: vec![("diacraft".into(), set(&["shared", "wayland-ish"])), ("diacraft-cli".into(), set(&["shared"]))],
            together: set(&["shared", "wayland-ish"]),
        },
        Build {
            triple: "wasm32-unknown-unknown".into(),
            label: "Web".into(),
            programs: vec![("diacraft-web".into(), set(&["shared", "wasm-glue"]))],
            together: set(&["shared", "wasm-glue"]),
        },
    ];
    (dir, inputs)
}

/// The policy names two files, the second for the web build alone, and every platform it lists is in one file: the downloads' platforms in the first and the
/// web in the second, with `diacraft-web` as the web target's only program.
#[test]
fn the_policy_has_one_file_for_the_downloads_and_one_for_the_web_build() {
    use xtask::licenses::policy::{OUTPUTS, TARGETS};
    assert_eq!(OUTPUTS.len(), 2);
    assert_eq!((OUTPUTS[0].file, OUTPUTS[1].file), ("THIRD-PARTY-LICENSES.txt", "THIRD-PARTY-LICENSES-WEB.txt"));
    assert_eq!(OUTPUTS[1].triples, ["wasm32-unknown-unknown"]);
    assert!(OUTPUTS[0].checks_lists && !OUTPUTS[1].checks_lists, "the lists are the downloads' file's");
    for (triple, label, programs) in TARGETS {
        let files: Vec<&str> = OUTPUTS.iter().filter(|o| o.triples.contains(triple)).map(|o| o.file).collect();
        assert_eq!(files.len(), 1, "{label} ({triple}) is in exactly one file: {files:?}");
        assert!(!programs.is_empty(), "{label} builds a program");
    }
    let web = TARGETS.iter().find(|(triple, _, _)| *triple == "wasm32-unknown-unknown").unwrap();
    assert_eq!((web.1, web.2), ("Web", &["diacraft-web"][..]));
    for output in OUTPUTS {
        for triple in output.triples {
            assert!(TARGETS.iter().any(|(t, _, _)| t == triple), "{triple} is a target");
        }
    }
}

#[test]
fn the_inputs_of_a_file_keep_the_builds_of_its_platforms_only() {
    let (_dir, inputs) = desktop_and_web_inputs();
    let web = inputs.only(&["wasm32-unknown-unknown"]);
    assert_eq!(web.builds.len(), 1);
    assert_eq!(web.builds[0].label, "Web");
    assert_eq!(inputs.only(&["x86_64-unknown-linux-gnu"]).builds[0].label, "Linux");
    assert!(inputs.only(&["no-such-triple"]).builds.is_empty());
    assert_eq!(web.packages.len(), inputs.packages.len(), "the packages are all kept: a build picks its own");
}

#[test]
fn the_web_file_lists_the_web_crates_only_and_says_it_is_the_web_builds() {
    let (_dir, inputs) = desktop_and_web_inputs();
    let (text, assembly) = xtask::licenses::notices_for(&inputs, &xtask::licenses::policy::OUTPUTS[1]).unwrap();
    let names: Vec<&str> = assembly.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["shared", "wasm-glue"], "no crate of the desktop build alone");
    assert!(text.starts_with("DiaCraft: third-party licences (the web build)\n"), "{text}");
    assert!(text.contains("Its web build, `diacraft-web`"), "{text}");
    assert!(text.contains("built into diacraft-web: Web\n"), "{text}");
    assert!(text.contains(&format!("{:<22} 2 crates (diacraft-web 2)", "Web")), "{text}");
    assert!(!text.contains("wayland-ish") && !text.contains("diacraft-cli") && !text.contains("libmimalloc"), "{text}");
    assert!(text.ends_with("End of the third-party licences.\n"));
    // The downloads' file is the other way round, and its first line is the one it always had.
    let (desktop, assembly) = xtask::licenses::notices_for(&inputs, &xtask::licenses::policy::OUTPUTS[0]).unwrap();
    let names: Vec<&str> = assembly.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["shared", "wayland-ish"]);
    assert!(desktop.starts_with("DiaCraft: third-party licences\n"), "{desktop}");
    assert!(desktop.contains("Its programs, `diacraft` and `diacraft-cli`"), "{desktop}");
    assert!(!desktop.contains("wasm-glue") && !desktop.contains("the web build"), "{desktop}");
}

/// An entry of a list in `policy.rs` that matches nothing is stale only for the file that checks the lists: the web file does not hold the desktop crates the
/// entries are about. A crate that compiles C code and is not in `NATIVE_CODE` is a problem in both.
#[test]
fn a_list_entry_for_a_desktop_crate_is_stale_in_the_downloads_file_and_not_in_the_web_file() {
    let (_dir, mut inputs) = registry(
        "web-lists",
        &[("libmimalloc-sys", Some("MIT"), &[("LICENSE", mit_text("2020 Alice"))]), ("shiny-sys", Some("MIT"), &[("LICENSE", mit_text("2020 Bob"))])],
    );
    // `libmimalloc-sys` is named in `NATIVE_CODE` and here has no build dependency on `cc`: stale where the lists are checked.
    let checked = assemble(&inputs);
    assert!(problems(&checked).contains("NATIVE_CODE lists libmimalloc-sys"), "{}", problems(&checked));
    let unchecked = xtask::licenses::assemble::assemble_with(&inputs, false);
    assert!(unchecked.problems.is_empty(), "{}", problems(&unchecked));
    // A crate that compiles C code, and that no list names, is refused either way.
    inputs.packages[1].native = true;
    for check_lists in [true, false] {
        let a = xtask::licenses::assemble::assemble_with(&inputs, check_lists);
        assert!(problems(&a).contains("shiny-sys") && problems(&a).contains("NATIVE_CODE"), "check_lists {check_lists}: {}", problems(&a));
    }
}

#[test]
fn a_file_whose_platforms_have_no_build_is_an_error_naming_it() {
    let (_dir, mut inputs) = desktop_and_web_inputs();
    inputs.builds.truncate(1);
    let problems = xtask::licenses::notices_for(&inputs, &xtask::licenses::policy::OUTPUTS[1]).unwrap_err();
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("THIRD-PARTY-LICENSES-WEB.txt") && problems[0].contains("wasm32-unknown-unknown"), "{}", problems[0]);
}

fn committed_web() -> String {
    std::fs::read_to_string(xtask::root().join("THIRD-PARTY-LICENSES-WEB.txt")).unwrap()
}

#[test]
fn the_committed_web_notices_name_the_web_program_and_hold_no_desktop_only_crate() {
    let text = committed_web();
    assert!(text.starts_with("DiaCraft: third-party licences (the web build)\n"), "{}", &text[..80]);
    for word in ["diacraft-web", "Web ", "wasm-bindgen", "web-sys", "js-sys", "wgpu", "The Rust standard library", "Hack-Regular.txt", "UFL.txt"] {
        assert!(text.contains(word), "the web notices do not say `{word}`");
    }
    // The desktop programs' crates: the C allocator, the window systems and the command line, none of which the browser build is built from.
    for word in ["diacraft-cli", "\nlibmimalloc-sys", "\nwayland-backend", "\nx11rb"] {
        assert!(!text.contains(word), "the web notices hold `{word}`, which is not in the web build");
    }
    assert!(text.len() > 50_000 && text.len() < 2 * 1024 * 1024, "{} bytes", text.len());
    assert!(text.ends_with("End of the third-party licences.\n"));
    // A line of every crate points at a text that exists.
    let texts = text
        .lines()
        .filter(|l| l.strip_prefix("Text ").and_then(|rest| rest.split_once(": ")).is_some_and(|(n, _)| n.parse::<usize>().is_ok()))
        .count();
    assert!(texts > 30, "{texts}");
    let mut crates = 0;
    for line in text.lines().filter(|l| l.starts_with("    licence ")) {
        crates += 1;
        let (_, numbers) = line.rsplit_once("text ").unwrap_or_else(|| panic!("{line}"));
        for n in numbers.split(", ") {
            let n: usize = n.trim().parse().unwrap_or_else(|_| panic!("{line}"));
            assert!((1..=texts).contains(&n), "{line}: text {n} of {texts}");
        }
    }
    assert!(crates > 100, "{crates}");
}
