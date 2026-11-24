//! `cargo xtask brands`: no other vendor's product names in what users and agents read. The scanner
//! is VectorCraft's, with DiaCraft's deny list. This file names the products on purpose: it sits in
//! `tests/`, which the scanner skips.

use xtask::brands::{
    BANNED_NAMES, DENY, Hit, Kind, MARK_ABOVE, NAME_EXCEPTIONS, PRODUCT_USES, TRADEMARK, allowed_in, kind_of, run, scan, scan_names, scan_text,
    scan_text_with, stale_uses,
};

fn hits(path: &str, text: &str) -> Vec<(usize, &'static str)> {
    scan_text(path, text).into_iter().map(|h| (h.line, h.term)).collect()
}

const RUST: &str = "crates/engine/src/cmd/file.rs";

#[test]
fn the_deny_list_is_microsoft_visio_and_lucid() {
    assert_eq!(DENY, ["microsoft", "visio", "lucidchart", "lucid"]);
}

#[test]
fn visio_in_a_string_literal_is_reported() {
    assert_eq!(hits(RUST, "pub const LABEL: &str = \"Open a Visio drawing\";\n"), [(1, "visio")]);
    assert_eq!(hits(RUST, "fn f() { let a = \"Made by Microsoft\"; let b = \"Lucidchart import\"; }\n"), [(1, "microsoft"), (1, "lucidchart")]);
    // Raw, byte and multi-line literals are read too, and a hit is placed on the literal's first line.
    let src = "fn f() { let r = r#\"raw \"Visio\" text\"#; let m = \"multi\nline Lucid\"; }\n";
    assert_eq!(hits(RUST, src), [(1, "visio"), (1, "lucid")]);
    let hit = &scan_text(RUST, "const A: &str = \"Visio\";")[0];
    assert_eq!((hit.path.as_str(), hit.line, hit.term, hit.context.as_str()), (RUST, 1, "visio", "Visio"));
}

#[test]
fn visio_in_a_comment_is_not() {
    let src = "// Visio in a line comment\n/// Microsoft in a doc comment\n/* block Visio /* nested */ Lucidchart */\nfn f() {}\n";
    assert!(hits(RUST, src).is_empty());
    // A comment's quotes do not open a literal either.
    let quoted = "// the label \"Visio\" is not shown\n/// A \"Microsoft\" doc comment\n/* \"Lucid\" /* \"Visio\" */ \"Visio\" */\nfn f() {}\n";
    assert!(hits(RUST, quoted).is_empty());
    // Code after a comment is scanned again.
    assert_eq!(hits(RUST, &format!("{quoted}const A: &str = \"Visio\"; // \"Visio\"\n")), [(5, "visio")]);
    // An identifier is not a literal, and a quote inside a char literal does not open one.
    assert!(hits(RUST, "fn visio_import() { let q = '\"'; let l: &'static str = \"fine\"; }\n").is_empty());
}

#[test]
fn visio_in_test_code_is_not() {
    let in_cfg_test =
        "#[cfg(test)]\nmod tests {\n    fn t() { let x = \"Visio\"; if true { let y = \"Microsoft\"; } }\n}\n#[cfg(test)]\nmod other;\n";
    assert!(hits(RUST, in_cfg_test).is_empty());
    // Code after a test module is scanned again.
    assert_eq!(hits(RUST, &format!("{in_cfg_test}fn g() {{ let z = \"Visio\"; }}\n")), [(7, "visio")]);
    for path in [
        "crates/engine/tests/it/main.rs",
        "crates/engine/src/tests.rs",
        "crates/engine/src/tests_file.rs",
        "crates/tools/src/text_tests.rs",
        "crates/engine/benches/open.rs",
        "xtask/tests/it/brands.rs",
    ] {
        assert!(hits(path, "const X: &str = \"Visio\";\n").is_empty(), "{path}");
        assert_eq!(kind_of(path), None, "{path}");
    }
}

/// A `#[cfg(test)]` on one field, variant or match arm ends with that item; it must not exempt the
/// next block, which is shipped code.
#[test]
fn a_cfg_test_field_does_not_exempt_the_next_block() {
    let shipped = "impl S {\n    fn label() -> &'static str { \"Open a Visio drawing\" }\n}\n";
    for fields in [
        "    #[cfg(test)]\n    pub hook: u32,\n",
        "    #[cfg(test)]\n    pub hook: u32\n",
        "    pub a: u8,\n    #[cfg(test)]\n    pub hook: u32,\n    pub b: u8,\n",
    ] {
        let src = format!("pub struct S {{\n{fields}}}\n{shipped}");
        let line = src.lines().position(|l| l.contains("Visio")).unwrap() + 1;
        assert_eq!(hits(RUST, &src), [(line, "visio")], "{fields}");
    }
}

#[test]
fn a_cfg_test_variant_does_not_exempt_the_next_block() {
    let shipped = "impl E {\n    fn label(&self) -> &'static str { \"Open a Visio drawing\" }\n}\n";
    for variants in [
        "    #[cfg(test)]\n    Hidden,\n    Shown,\n",
        "    Shown,\n    #[cfg(test)]\n    Hidden\n",
        "    #[cfg(test)]\n    Hidden(u8, u8),\n    Shown,\n",
    ] {
        let src = format!("pub enum E {{\n{variants}}}\n{shipped}");
        let line = src.lines().position(|l| l.contains("Visio")).unwrap() + 1;
        assert_eq!(hits(RUST, &src), [(line, "visio")], "{variants}");
    }
}

#[test]
fn a_cfg_test_match_arm_does_not_exempt_the_next_block() {
    let shipped = "fn g() -> &'static str { \"Open a Visio drawing\" }\n";
    for arms in [
        "        #[cfg(test)]\n        0 => 1,\n        _ => 2,\n",
        "        _ => 2,\n        #[cfg(test)]\n        0 => 1\n",
        "        #[cfg(test)]\n        0 => f(1, 2),\n        _ => 2,\n",
    ] {
        let src = format!("fn f(x: u8) -> u8 {{\n    match x {{\n{arms}    }}\n}}\n{shipped}");
        let line = src.lines().position(|l| l.contains("Visio")).unwrap() + 1;
        assert_eq!(hits(RUST, &src), [(line, "visio")], "{arms}");
    }
}

/// What a cfg(test) attribute still exempts: the block of the item it sits on, however it is spelled.
#[test]
fn a_cfg_test_item_still_exempts_its_own_block() {
    let spellings = [
        "#[cfg(test)]\nmod tests {\n    fn t() { let x = \"Visio\"; }\n}\n",
        "#[cfg(test)]\n#[derive(Debug, Clone)]\nstruct T { label: &'static str }\n#[cfg(test)]\nimpl T { fn l() -> &'static str { \"Visio\" } }\n",
        "#[cfg(test)]\nfn helper(a: u8, b: u8) -> &'static str { if a > b { \"Visio\" } else { \"x\" } }\n",
        "struct S {\n    a: u8,\n}\n#[cfg(test)]\nimpl S {\n    fn l() -> &'static str { \"Visio\" }\n}\n",
    ];
    for src in spellings {
        assert!(hits(RUST, src).is_empty(), "{src}");
    }
}

#[test]
fn brand_ok_marker_allows_a_line() {
    assert!(hits(RUST, "const OLD: &str = \"Visio Drawing\"; // brand-ok: the file type's own name\n").is_empty());
    // The marker covers its own line only.
    assert_eq!(hits(RUST, "const A: &str = \"Visio\"; // brand-ok\nconst B: &str = \"Visio\";\n"), [(2, "visio")]);
    // And in text files and manifests.
    assert!(hits("packaging/x.txt", "Keywords=diagram;visio; # brand-ok\n").is_empty());
    assert_eq!(hits("packaging/x.txt", "Like Microsoft apps\nnot here: Microsoft # brand-ok\n"), [(1, "microsoft")]);
}

#[test]
fn a_hash_line_marks_the_line_below_in_the_listed_packaging_files_only() {
    let desktop = "# shared type (brand-ok)\nMimeType=application/vnd.visio;\n# a comment\nMimeType=application/vnd.visio;\n";
    assert_eq!(hits(MARK_ABOVE[0], desktop), [(4, "visio")], "a `#` line marks the line below it only");
    assert_eq!(hits("packaging/linux/other.desktop", desktop), [(2, "visio"), (4, "visio")], "only `MARK_ABOVE` files take a mark from above");
}

#[test]
fn markdown_docs_are_not_scanned() {
    for path in [
        "README.md",
        "ROADMAP.md",
        "NOTES.md",
        "AGENTS.md",
        "ATTRIBUTION.md",
        "docs/gaps.md",
        "docs/mcp.md",
        "notes/formulas.md",
        "packaging/README.md",
    ] {
        assert_eq!(kind_of(path), None, "{path}");
        assert!(hits(path, "Works like Visio and Microsoft Visio.\n").is_empty(), "{path}");
    }
}

#[test]
fn whole_words_only_and_case_is_ignored() {
    assert_eq!(hits("packaging/x.txt", "Microsoft's app\n"), [(1, "microsoft")]);
    assert_eq!(hits("packaging/x.txt", "a VISIO-style UI\n"), [(1, "visio")]);
    assert!(hits("packaging/x.txt", "revision supervision visions microsoftware lucidly\n").is_empty());
    // `lucid` is a whole word of its own; `lucidchart` is another.
    assert_eq!(hits("packaging/x.txt", "lucid and Lucidchart\n"), [(1, "lucidchart"), (1, "lucid")]);
}

#[test]
fn manifests_are_scanned_for_their_description_only() {
    let manifest = "name = \"visio-thing\"\ndescription = \"Reads Visio files\"\n";
    assert_eq!(hits("crates/x/Cargo.toml", manifest), [(2, "visio")]);
    assert_eq!(hits("Cargo.toml", manifest), [(2, "visio")]);
}

#[test]
fn scope() {
    assert_eq!(kind_of(RUST), Some(Kind::Rust));
    assert_eq!(kind_of("apps/diacraft-cli/src/main.rs"), Some(Kind::Rust));
    assert_eq!(kind_of("xtask/src/ci.rs"), Some(Kind::Rust));
    // The scanner lists the names it rejects.
    assert_eq!(kind_of("xtask/src/brands.rs"), None);
    assert_eq!(kind_of("crates/ui-egui/Cargo.toml"), Some(Kind::Manifest));
    assert_eq!(kind_of("docs/notice.txt"), Some(Kind::Text));
    assert_eq!(kind_of("packaging/linux/nfpm.yaml"), Some(Kind::Text));
    // A Kani harness file is not shipped code: no ordinary build compiles it, and its `cover!` messages and assertions are not user-visible text.
    assert_eq!(kind_of("crates/calc/src/kani_proofs.rs"), None);
    assert_eq!(kind_of("crates/units/src/kani_proofs.rs"), None);
    assert_eq!(kind_of("crates/calc/src/kani_proofs_extra.rs"), Some(Kind::Rust), "only the harness file's own name is skipped");
    for skipped in [".github/workflows/ci.yml", "notes/notes.txt", "build/gen.rs", "deny.toml"] {
        assert_eq!(kind_of(skipped), None, "{skipped}");
    }
}

/// A fuzz crate (`crates/*/fuzz/`) is tool code, not a test: its sources are scanned, so a part in the package's folder is named through
/// the testkit's `package::DIR` (which the VSDX fuzz crate includes by path), not as a literal. A seed of the VSDX fuzz targets that wrote
/// its picture part as a literal turned `the_tree_is_clean` red.
#[test]
fn a_fuzz_crates_sources_are_scanned() {
    for path in ["crates/vsdx/fuzz/src/seeds.rs", "crates/vsdx/fuzz/fuzz_targets/vsdx_read.rs", "crates/native/fuzz/src/lib.rs"] {
        assert_eq!(kind_of(path), Some(Kind::Rust), "{path}");
        assert_eq!(hits(path, "fn f() { p.part(\"/visio/media/i.png\", \"image/png\", b); }\n"), [(1, "visio")], "{path}");
        assert!(hits(path, "fn f() { p.part(&format!(\"{DIR}/media/i.png\"), \"image/png\", b); }\n").is_empty(), "{path}");
    }
}

#[test]
fn injected_strings_fail_the_gate() {
    let dir = crate::TempDir::new("brands");
    dir.write("crates/x/src/lib.rs", "pub const LABEL: &str = \"Export for Visio\"; // a comment: Microsoft\n");
    dir.write("docs/x.md", "Works like Visio.\n");
    dir.write("docs/x.txt", "Works like Visio.\n");
    dir.write("packaging/x.desktop", "Keywords=diagram;visio;\n");
    let marked = "# shared type (brand-ok)\nMimeType=application/vnd.visio;\n";
    dir.write(MARK_ABOVE[0], marked);
    dir.write("packaging/x.yaml", marked);
    dir.write("README.md", "Microsoft trademark notice\n");
    let files: Vec<String> = [
        "crates/x/src/lib.rs",
        "docs/x.md",
        "docs/x.txt",
        "packaging/x.desktop",
        MARK_ABOVE[0],
        "packaging/x.yaml",
        "README.md",
        // Not scanned, so not read: a missing file of an unscanned kind is no error.
        "build/missing.rs",
    ]
    .map(String::from)
    .to_vec();
    let found: Vec<(String, usize, &str)> = scan(dir.path(), &files).unwrap().into_iter().map(|h| (h.path, h.line, h.term)).collect();
    assert_eq!(
        found,
        [
            ("crates/x/src/lib.rs".to_string(), 1, "visio"),
            ("docs/x.txt".to_string(), 1, "visio"),
            ("packaging/x.desktop".to_string(), 1, "visio"),
            ("packaging/x.yaml".to_string(), 2, "visio"),
        ]
    );
}

fn files(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_string()).collect()
}

/// Text files in packaging may be in another encoding; the vendor name is still ASCII.
#[test]
fn a_latin1_text_file_is_still_scanned() {
    let dir = crate::TempDir::new("brands-latin1");
    dir.write_bytes("packaging/linux/x.desktop", b"Name=Dia\xe9\nKeywords=diagram;visio;\n");
    dir.write_bytes("docs/notes.txt", b"caf\xe9 Microsoft\n");
    let found = scan(dir.path(), &files(&["packaging/linux/x.desktop", "docs/notes.txt"])).unwrap();
    let found: Vec<(&str, usize, &str)> = found.iter().map(|h| (h.path.as_str(), h.line, h.term)).collect();
    assert_eq!(found, [("packaging/linux/x.desktop", 2, "visio"), ("docs/notes.txt", 1, "microsoft")]);
}

#[test]
fn a_rust_or_manifest_file_that_is_not_utf8_is_an_error() {
    let dir = crate::TempDir::new("brands-utf8");
    dir.write_bytes("crates/x/src/lib.rs", b"pub const A: &str = \"caf\xe9 Visio\";\n");
    dir.write_bytes("crates/x/Cargo.toml", b"description = \"caf\xe9 Visio\"\n");
    for path in ["crates/x/src/lib.rs", "crates/x/Cargo.toml"] {
        let err = scan(dir.path(), &files(&[path])).unwrap_err();
        assert!(err.contains(path) && err.contains("UTF-8"), "{err}");
    }
}

#[test]
fn a_listed_file_that_cannot_be_read_is_an_error() {
    let dir = crate::TempDir::new("brands-unreadable");
    // Listed but not there (git lists only files that exist or are known deletions).
    let err = scan(dir.path(), &files(&["crates/x/src/missing.rs"])).unwrap_err();
    assert!(err.contains("crates/x/src/missing.rs") && err.contains("cannot read"), "{err}");
    // A directory where a text file is expected (a submodule is listed this way).
    std::fs::create_dir_all(dir.path().join("packaging/odd.txt")).unwrap();
    assert!(scan(dir.path(), &files(&["packaging/odd.txt"])).is_err());
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_and_an_unreadable_file_are_errors() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("brands-links");
    std::fs::create_dir_all(dir.path().join("packaging")).unwrap();
    std::os::unix::fs::symlink("nowhere", dir.path().join("packaging/link.desktop")).unwrap();
    let err = scan(dir.path(), &files(&["packaging/link.desktop"])).unwrap_err();
    assert!(err.contains("packaging/link.desktop"), "{err}");

    dir.write("packaging/secret.desktop", "Keywords=visio;\n");
    let path = dir.path().join("packaging/secret.desktop");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything: there is no permission error to test then.
    if std::fs::read(&path).is_err() {
        let err = scan(dir.path(), &files(&["packaging/secret.desktop"])).unwrap_err();
        assert!(err.contains("packaging/secret.desktop"), "{err}");
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn files_with_non_ascii_or_spaced_names_are_scanned() {
    let dir = crate::git_repo("brands-names");
    dir.write("crates/x/src/naïve module.rs", "pub const A: &str = \"Export for Visio\";\n");
    dir.write("packaging/linux/Ünïcode app.desktop", "Keywords=diagram;visio;\n");
    let files = xtask::repo_files(dir.path()).unwrap();
    let found: Vec<(String, usize, &str)> = scan(dir.path(), &files).unwrap().into_iter().map(|h| (h.path, h.line, h.term)).collect();
    assert_eq!(found, [("crates/x/src/naïve module.rs".to_string(), 1, "visio"), ("packaging/linux/Ünïcode app.desktop".to_string(), 1, "visio")]);
}

#[test]
fn the_tree_is_clean() {
    let root = xtask::root();
    let files = xtask::repo_files(&root).unwrap();
    let hits = scan(&root, &files).unwrap();
    assert!(hits.is_empty(), "{hits:#?}");
}

// ---------- the suite's names (AGENTS.md: no other project's brand, link or app id anywhere) ----------

/// The four banned strings, written from pieces so that this file does not hold them: the scan of names reads every tracked
/// file, tests included, and a file that spells them out would need an exception of its own.
fn banned() -> [String; 4] {
    [["art", "craft"].concat(), ["get", "art", "craft"].concat(), ["ai", ".", "story", "teller"].concat(), ["discord", ".gg"].concat()]
}

/// The vendor scan reads string literals, manifest descriptions, `docs/` and `packaging/`. The ban on another project's names
/// reaches further: any text in any tracked file, whatever its kind and case, in comments and in tests. A Rust literal that had
/// the three names (and nothing else) passed `cargo xtask brands`.
#[test]
fn another_projects_names_in_any_tracked_file_fail_the_gate() {
    let [art, getart, id, discord] = banned();
    let dir = crate::git_repo("brands-names-run");
    dir.write("crates/x/src/lib.rs", &format!("pub const LABEL: &str = \"{} {getart} {id}\";\n", art.to_uppercase()));
    let err = run(dir.path()).unwrap_err();
    for term in [&art, &getart, &id] {
        assert!(err.contains(term.as_str()), "{term}: {err}");
    }
    assert!(err.contains("crates/x/src/lib.rs:1"), "{err}");

    // Anywhere else too: a comment, a test, a manifest, a workflow, a script, a page, a file with no extension.
    for (path, text) in [
        ("crates/x/src/lib.rs", format!("// from {art}\n")),
        ("crates/x/tests/it/main.rs", format!("const X: &str = \"{id}\";\n")),
        ("crates/x/Cargo.toml", format!("[package]\nhomepage = \"https://{getart}.com\"\n")),
        (".github/workflows/ci.yml", format!("name: CI\n# see https://{discord}/x\n")),
        ("scripts/run.sh", format!("echo {art}\n")),
        ("apps/x/index.html", format!("<a href=\"https://{discord}/x\">chat</a>\n")),
        ("LICENSE-NOTE", format!("{art} Team\n")),
        ("docs/x.md", format!("A line.\n{art} again.\n")),
    ] {
        let dir = crate::git_repo("brands-names-anywhere");
        dir.write(path, &text);
        let err = run(dir.path()).unwrap_err();
        assert!(err.contains(path), "{path}: {err}");
    }
}

/// Case is not a way round it: each name in lower, upper and mixed case alone is a hit.
#[test]
fn the_case_of_a_banned_name_does_not_matter() {
    let dir = crate::TempDir::new("brands-case");
    for (i, name) in banned().iter().enumerate() {
        let mixed: String = name.chars().enumerate().map(|(n, c)| if n % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
        for (j, spelling) in [name.clone(), name.to_uppercase(), mixed].into_iter().enumerate() {
            let path = format!("case{i}{j}.txt");
            dir.write(&path, &format!("x {spelling} y\n"));
            let names = scan_names(dir.path(), &files(&[path.as_str()]), BANNED_NAMES, &[]).unwrap();
            assert!(names.hits.iter().any(|h| h.term == BANNED_NAMES[i]), "{spelling}: {:?}", names.hits);
        }
    }
}

#[test]
fn the_banned_names_are_the_four_the_rule_names() {
    assert_eq!(BANNED_NAMES.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>(), banned());
}

/// A line that holds a name is covered by an exception of its own file whose text it contains, and by nothing else: not the
/// same text in another file, not another line of the same file.
#[test]
fn an_exception_covers_its_own_file_and_line_and_no_other() {
    let [art, _, id, _] = banned();
    let dir = crate::TempDir::new("brands-exceptions");
    let said = format!("DiaCraft is not an {art} product.");
    dir.write("docs/a.md", &format!("{said}\nA line that is fine.\nSee {art} for more.\n"));
    dir.write("docs/b.md", &format!("{said}\n"));
    dir.write("docs/c.md", &format!("Uses {id} ids.\n"));
    let all = files(&["docs/a.md", "docs/b.md", "docs/c.md"]);
    let exceptions = [("docs/a.md", "is not an")];
    let names = scan_names(dir.path(), &all, BANNED_NAMES, &exceptions).unwrap();
    let place = |hits: &[Hit]| hits.iter().map(|h| (h.path.clone(), h.line, h.term)).collect::<Vec<_>>();
    assert_eq!(place(&names.exempt), [("docs/a.md".to_owned(), 1, BANNED_NAMES[0])]);
    assert_eq!(
        place(&names.hits),
        [("docs/a.md".to_owned(), 3, BANNED_NAMES[0]), ("docs/b.md".to_owned(), 1, BANNED_NAMES[0]), ("docs/c.md".to_owned(), 1, BANNED_NAMES[2])]
    );
    assert!(names.stale.is_empty());
    // No exceptions: all four lines are hits.
    assert_eq!(scan_names(dir.path(), &all, BANNED_NAMES, &[]).unwrap().hits.len(), 4);
    // Every banned name found on a line is its own hit: the site's name holds the brand's, so it is both.
    dir.write("docs/d.md", &format!("https://{}.com\n", BANNED_NAMES[1]));
    let names = scan_names(dir.path(), &files(&["docs/d.md"]), BANNED_NAMES, &[]).unwrap();
    assert_eq!(names.hits.iter().map(|h| h.term).collect::<Vec<_>>(), [BANNED_NAMES[0], BANNED_NAMES[1]]);
}

/// An exception whose file is there and whose text is not is stale (the line was reworded, and the entry would now bless
/// whatever a later edit puts under the same words). One whose file is not there is not: the list is for this repository,
/// and a scratch repository has none of its files.
#[test]
fn a_stale_exception_is_reported_when_its_file_is_there() {
    let [art, ..] = banned();
    let dir = crate::git_repo("brands-stale");
    dir.write("NOTE.md", "Nothing to see.\n");
    let names = scan_names(dir.path(), &files(&["NOTE.md"]), BANNED_NAMES, &[("NOTE.md", "was a statement"), ("MISSING.md", "x")]).unwrap();
    assert_eq!(names.stale, [("NOTE.md".to_owned(), "was a statement".to_owned())]);
    assert!(names.hits.is_empty() && names.exempt.is_empty());
    // Through the gate: the real list names AGENTS.md, which this repository does not have, and passes; with the file and no
    // statement in it, the gate fails and says which entry.
    dir.write("clean.txt", "x\n");
    run(dir.path()).unwrap();
    dir.write("AGENTS.md", "A different line.\n");
    let err = run(dir.path()).unwrap_err();
    assert!(err.contains("NAME_EXCEPTIONS match no line") && err.contains("AGENTS.md: no line contains"), "{err}");
    dir.write("AGENTS.md", &format!("DiaCraft is an independent project, not an {} product.\n", ["Art", "Craft"].concat()));
    let err = run(dir.path()).unwrap_err();
    assert!(err.contains("AGENTS.md: no line contains \"No "), "the second entry for AGENTS.md is stale: {err}");
    assert!(!err.contains(&format!("AGENTS.md:1: \"{art}\"")), "the covered line is not a hit: {err}");
}

/// The lines this repository lets through, by file: what the list exempts, which is part of what a reviewer signs off. A new
/// exception, or a covered file that grows a line, changes this and has to be said here.
#[test]
fn the_tree_has_no_name_but_in_the_lines_the_list_exempts() {
    let root = xtask::root();
    let files = xtask::repo_files(&root).unwrap();
    let names = scan_names(&root, &files, BANNED_NAMES, NAME_EXCEPTIONS).unwrap();
    assert!(names.hits.is_empty(), "{:#?}", names.hits);
    assert!(names.stale.is_empty(), "{:?}", names.stale);
    let mut lines: Vec<(String, usize)> = names.exempt.iter().map(|h| (h.path.clone(), h.line)).collect();
    lines.dedup();
    let mut by_file: Vec<(String, usize)> = Vec::new();
    for (path, _) in &lines {
        match by_file.last_mut() {
            Some((last, n)) if last == path => *n += 1,
            _ => by_file.push((path.clone(), 1)),
        }
    }
    assert_eq!(
        by_file,
        [
            ("AGENTS.md".to_owned(), 2),
            ("NOTICE".to_owned(), 1),
            ("THIRD-PARTY-LICENSES-WEB.txt".to_owned(), 1),
            ("THIRD-PARTY-LICENSES.txt".to_owned(), 2),
            ("apps/diacraft-web/tests/it/html.rs".to_owned(), 1),
        ]
    );
}

/// A file the scan cannot read whole is an error: a gate that skips a file it cannot see has not seen it. A name in a file that is
/// not UTF-8 is found all the same.
#[test]
fn a_file_too_big_to_read_is_an_error_and_a_latin1_file_is_scanned() {
    let [art, ..] = banned();
    let dir = crate::TempDir::new("brands-big");
    let big = std::fs::File::create(dir.path().join("big.bin")).unwrap();
    big.set_len(33 * 1024 * 1024).unwrap();
    let err = scan_names(dir.path(), &files(&["big.bin"]), BANNED_NAMES, &[]).unwrap_err();
    assert!(err.contains("big.bin") && err.contains("more than"), "{err}");
    let err = scan_names(dir.path(), &files(&["missing.txt"]), BANNED_NAMES, &[]).unwrap_err();
    assert!(err.contains("missing.txt") && err.contains("cannot read"), "{err}");
    dir.write_bytes("latin1.txt", &[b"caf\xe9 ".as_slice(), art.as_bytes(), b"\n"].concat());
    let names = scan_names(dir.path(), &files(&["latin1.txt"]), BANNED_NAMES, &[]).unwrap();
    assert_eq!(names.hits.len(), 1);
}

/// The export's allowlist copies, whole, lines that hold a banned name (the copyright lines a licence keeps, the gate's own list): each
/// of those lines is scanned in its own file, so the list itself is not scanned, as this gate's own file is not. The same line elsewhere is.
#[test]
fn the_exports_allowlist_is_not_scanned_for_the_names_and_another_file_is() {
    let [art, ..] = banned();
    let dir = crate::TempDir::new("brands-allowlist");
    let line = format!("NOTICE :: Copyright (c) 2026 {art} Team\n");
    dir.write(xtask::brands::NAME_LISTS[0], &line);
    dir.write("xtask/src/export/other.txt", &line);
    let names = scan_names(dir.path(), &files(&[xtask::brands::NAME_LISTS[0], "xtask/src/export/other.txt"]), BANNED_NAMES, &[]).unwrap();
    assert_eq!(names.hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), ["xtask/src/export/other.txt"]);
}

#[test]
fn a_banned_name_in_a_kani_harness_file_is_not_reported_and_in_the_crate_next_to_it_is() {
    let dir = crate::TempDir::new("brands-kani");
    dir.write("crates/x/src/kani_proofs.rs", "fn proof() { kani::cover!(true, \"Visio\"); }\n");
    dir.write("crates/x/src/lib.rs", "pub const LABEL: &str = \"Visio\";\n");
    let files = vec!["crates/x/src/kani_proofs.rs".to_string(), "crates/x/src/lib.rs".to_string()];
    let paths: Vec<String> = scan(dir.path(), &files).unwrap().into_iter().map(|h| h.path).collect();
    assert_eq!(paths, ["crates/x/src/lib.rs"]);
}

// ---------- the uses the owner allows (owner, 2026-11-23: "Labels + site/docs format lists") ----------

const SAVE_KINDS: &str = "crates/ui-egui/src/save_kinds.rs";
const ABOUT: &str = "crates/ui-egui/src/splash.rs";

/// A label the owner allows passes, as the literal it is, in the file that shows it. The name in other words, in an identifier, in
/// another case, beside the label in the same literal or in a file the label is not listed for is a hit as before.
#[test]
fn a_label_passes_and_the_name_in_other_words_or_an_identifier_fails() {
    let label = "Visio Drawing (.vsdx)";
    assert!(allowed_in(SAVE_KINDS).contains(&label));
    assert!(hits(SAVE_KINDS, &format!("const KIND: SaveKind = SaveKind {{ label: \"{label}\", extension: \"vsdx\" }};\n")).is_empty());
    for (literal, why) in [
        ("Save it as a Visio drawing", "other words"),
        ("file.save_visio", "an identifier"),
        ("visio_drawing", "an identifier"),
        ("visio drawing (.vsdx)", "another case"),
        ("Visio Drawing (.vsdx) for Visio", "the name again beside the label"),
        ("Microsoft Visio Drawing (.vsdx)", "the company beside the label"),
    ] {
        assert_eq!(hits(SAVE_KINDS, &format!("const L: &str = \"{literal}\";\n")).len(), 1, "{why}: {literal}");
    }
    // The label in a file it is not listed for.
    assert_eq!(hits(RUST, &format!("const L: &str = \"{label}\";\n")), [(1, "visio")]);
    // The list given: what it names passes in any file, and nothing else does.
    assert!(scan_text_with(RUST, &format!("const L: &str = \"{label}\";\n"), &[label]).is_empty());
    assert_eq!(scan_text_with(RUST, "const L: &str = \"Visio Stencil (.vssx)\";\n", &[label]).len(), 1);
}

/// The trademark line passes in the About box, and both names in it with it; the company's name elsewhere in the same file is a hit.
#[test]
fn the_trademark_line_passes_where_it_is_listed_and_nowhere_else() {
    assert!(TRADEMARK.contains("Visio") && TRADEMARK.contains("Microsoft"));
    assert!(hits(ABOUT, &format!("pub const TRADEMARK: &str = \"{TRADEMARK}\";\n")).is_empty());
    assert_eq!(hits(ABOUT, "const MADE: &str = \"Made by Microsoft\";\n"), [(1, "microsoft")]);
    assert_eq!(hits(RUST, &format!("const T: &str = \"{TRADEMARK}\";\n")), [(1, "microsoft"), (1, "visio")]);
    for page in ["site/index.html", "site/privacy.html", "site/guides/agents.html", "site/404.html"] {
        assert!(hits(page, &format!("<footer><p class=\"legal\">{TRADEMARK}</p></footer>\n")).is_empty(), "{page}");
        assert_eq!(hits(page, "<p>Works with Microsoft Visio.</p>\n").len(), 2, "{page}");
    }
}

/// The notice (owner, 2026-11-23: "a note about the OASIS OpenDocument format and that we are not affiliated with OASIS, LibreOffice or OpenOffice"): its first sentence is the one it has always
/// had (the allowed use of the product's name); it says that OpenDocument Format is an OASIS Standard, whose marks LibreOffice and OpenOffice are, and that DiaCraft is not affiliated with or
/// endorsed by the four owners. The text is one paragraph, in NOTICE, the About box and every page's footer (`the_tree_holds_every_use_the_list_names` holds each place to it).
#[test]
fn the_notice_keeps_its_first_sentence_and_names_the_owners_of_the_other_marks() {
    assert!(TRADEMARK.starts_with("Visio is a trademark of the Microsoft group of companies. "), "{TRADEMARK}");
    for sentence in [
        "OpenDocument Format (ODF) is an OASIS Standard.",
        "LibreOffice is a registered trademark of The Document Foundation.",
        "OpenOffice and Apache OpenOffice are registered trademarks of The Apache Software Foundation.",
        "DiaCraft is not affiliated with or endorsed by Microsoft, OASIS, The Document Foundation or The Apache Software Foundation.",
    ] {
        assert!(TRADEMARK.contains(sentence), "{sentence}");
    }
    assert!(
        TRADEMARK.ends_with("endorsed by Microsoft, OASIS, The Document Foundation or The Apache Software Foundation."),
        "it ends with who it is not affiliated with"
    );
    assert!(!TRADEMARK.contains('\n') && TRADEMARK.is_ascii(), "one line of plain text, which the allowlist keeps whole");
    // Only the first and the last sentence hold a name of the deny list, and both are inside the allowed text; the others hold none, so they pass in a Rust literal anywhere.
    let others = "OpenDocument Format (ODF) is an OASIS Standard. LibreOffice is a registered trademark of The Document Foundation. OpenOffice and Apache OpenOffice are registered \
                  trademarks of The Apache Software Foundation.";
    assert!(TRADEMARK.contains(others));
    assert!(hits(RUST, &format!("const T: &str = \"{others}\";\n")).is_empty());
    // The first sentence of the notice alone is not the allowed text: the use is the whole notice, in the files it is listed for.
    assert_eq!(hits(ABOUT, "const T: &str = \"Visio is a trademark of the Microsoft group of companies.\";\n"), [(1, "microsoft"), (1, "visio")]);
}

/// The website's text files are scanned as text, and its pictures are not.
#[test]
fn the_sites_text_files_are_scanned_and_its_pictures_are_not() {
    for path in [
        "site/index.html",
        "site/guides/file-formats.html",
        "site/styles.css",
        "site/download.js",
        "site/site.json",
        "site/robots.txt",
        "site/sitemap.xml",
        "site/site.webmanifest",
        "site/_headers",
    ] {
        assert_eq!(kind_of(path), Some(Kind::Text), "{path}");
    }
    for path in ["site/assets/social.jpg", "site/assets/icon-32.png", "site/assets/hashed/octopus.4c9f9c16802f.webp"] {
        assert_eq!(kind_of(path), None, "{path}");
    }
    let list = allowed_in("site/index.html")[0];
    assert!(hits("site/index.html", &format!("<dd>{list} (<code>.vsdx</code>)</dd>\n")).is_empty());
    assert_eq!(hits("site/changelog.html", &format!("<dd>{list}</dd>\n")), [(1, "visio")], "the home page's list is the home page's");
}

/// An entry whose file is there and does not hold its text is stale, and fails the gate; one whose file is not there is not.
#[test]
fn a_use_whose_file_does_not_hold_it_is_stale() {
    let dir = crate::git_repo("brands-uses");
    dir.write("site/privacy.html", "<p>DiaCraft</p>\n");
    dir.write("other.txt", "x\n");
    let all = files(&["site/privacy.html", "other.txt"]);
    assert_eq!(
        stale_uses(dir.path(), &all, &[("site/privacy.html", TRADEMARK), ("MISSING", "x")]).unwrap(),
        [("site/privacy.html".to_owned(), TRADEMARK.to_owned())]
    );
    let err = run(dir.path()).unwrap_err();
    assert!(err.contains("PRODUCT_USES are not in their files") && err.contains("site/privacy.html: does not hold"), "{err}");
    // The page with the line in its footer passes, and the name in other words of it does not.
    dir.write("site/privacy.html", &format!("<p>DiaCraft</p>\n<p class=\"legal\">{TRADEMARK}</p>\n"));
    run(dir.path()).unwrap();
    dir.write("site/privacy.html", &format!("<p>DiaCraft for Visio</p>\n<p class=\"legal\">{TRADEMARK}</p>\n"));
    let err = run(dir.path()).unwrap_err();
    assert!(err.contains("site/privacy.html:1: \"visio\""), "{err}");
}

/// Every use the list names is in its file in this repository.
#[test]
fn the_tree_holds_every_use_the_list_names() {
    let root = xtask::root();
    let files = xtask::repo_files(&root).unwrap();
    for (path, _) in PRODUCT_USES {
        assert!(files.iter().any(|f| f == path), "{path} is not in the repository");
    }
    assert_eq!(stale_uses(&root, &files, PRODUCT_USES).unwrap(), Vec::<(String, String)>::new());
}
