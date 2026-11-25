//! Tests of the web app's module in parts and of the files beside it: where the module is cut, the manifest, the preloads, and what the check refuses.

use super::app;
use super::app_tests::{Dir, EMPTY_WASM, HEADERS, LOADER, glue_out, module, out_dir_with, repo, settings};
use super::parts::{self, ASSETS, MAX_MODULE, MAX_PARTS, MODULE_BUDGET, PART_LIMIT};
use serde_json::{Value, json};
use std::fs;

// ----------------------------------------------------------------------------------------------------------------------------------- the cut

#[test]
fn sha256_is_the_standard_one() {
    assert_eq!(parts::sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(parts::sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

/// What a cut must be for every length: contiguous from 0 to the end, none empty, none over the limit, near-equal (all the same size but the last, which is no larger).
fn assert_a_cut(len: u64) {
    let ranges = parts::plan(len).unwrap();
    assert!(!ranges.is_empty() && ranges.len() <= MAX_PARTS, "{len}: {} parts", ranges.len());
    let mut at = 0usize;
    for range in &ranges {
        assert_eq!(range.start, at, "{len}: contiguous");
        assert!(range.end > range.start, "{len}: no part is empty");
        assert!((range.end - range.start) as u64 <= PART_LIMIT, "{len}: a part is over the limit");
        at = range.end;
    }
    assert_eq!(at as u64, len, "{len}: together, the module");
    let first = ranges[0].end - ranges[0].start;
    for range in &ranges[..ranges.len() - 1] {
        assert_eq!(range.end - range.start, first, "{len}: all the same but the last");
    }
    let last = ranges[ranges.len() - 1].clone();
    assert!(last.end - last.start <= first, "{len}: the last is no larger");
    // As few parts as keep each within the limit.
    assert_eq!(ranges.len() as u64, len.div_ceil(PART_LIMIT), "{len}");
}

#[test]
fn a_module_is_cut_into_the_fewest_parts_of_near_equal_size_that_are_each_within_the_limit() {
    for len in [
        1,
        2,
        7,
        1000,
        PART_LIMIT - 1,
        PART_LIMIT,
        PART_LIMIT + 1,
        2 * PART_LIMIT,
        2 * PART_LIMIT + 1,
        17_400_000,
        21_008_642,
        3 * PART_LIMIT,
        MAX_MODULE - 1,
        MAX_MODULE,
    ] {
        assert_a_cut(len);
    }
    // A pseudo-random sweep: the same properties at lengths nobody chose.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..2000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        assert_a_cut(1 + x % MAX_MODULE);
    }
    assert_eq!(parts::plan(PART_LIMIT).unwrap().len(), 1, "a module that is exactly the limit is one part");
    assert_eq!(parts::plan(PART_LIMIT + 1).unwrap().len(), 2);
    assert_eq!(parts::plan(21_008_642).unwrap().len(), 3, "a module of 20 MiB is three parts of about 6.7 MiB");
}

#[test]
fn an_empty_module_or_one_the_loader_would_refuse_is_not_cut() {
    assert!(parts::plan(0).unwrap_err().contains("empty"));
    assert!(parts::plan(MAX_MODULE + 1).unwrap_err().contains("at most"));
    assert!(parts::plan(u64::MAX).is_err());
    // The module the loader reads is at most 64 MiB, and sixteen parts of 8 MiB are twice that: a cut never runs out of parts before it runs out of module.
    assert_eq!(MAX_MODULE * 2, MAX_PARTS as u64 * PART_LIMIT);
}

// ----------------------------------------------------------------------------------------------------------------------------- the page

const HEAD: &str = "<head><link rel=\"icon\" href=\"./i.png\"><link rel=\"modulepreload\" href=\"./g.js\" crossorigin=\"anonymous\"><link rel=\"preload\" href=\"./g_bg.wasm\" crossorigin=\"anonymous\" integrity=\"sha384-x\" as=\"fetch\" type=\"application/wasm\"></head><body></body>";

#[test]
fn the_page_preloads_the_manifest_and_each_part_and_not_the_module_whole() {
    let files = vec!["parts/diacraft-web-1-aa.wasm".to_owned(), "parts/diacraft-web-2-bb.wasm".to_owned()];
    let page = parts::preloads(HEAD, &files).unwrap();
    assert!(!page.contains("g_bg.wasm"), "Trunk's preload of the module is gone: {page}");
    assert!(
        page.contains("<link rel=\"icon\" href=\"./i.png\"><link rel=\"modulepreload\" href=\"./g.js\" crossorigin=\"anonymous\">"),
        "the others stay: {page}"
    );
    assert!(
        page.contains(
            "<link rel=\"preload\" href=\"./manifest.json\" as=\"fetch\" crossorigin=\"anonymous\"><link rel=\"preload\" href=\"./parts/diacraft-web-1-aa.wasm\" as=\"fetch\" crossorigin=\"anonymous\"><link rel=\"preload\" href=\"./parts/diacraft-web-2-bb.wasm\" as=\"fetch\" crossorigin=\"anonymous\"></head>"
        ),
        "{page}"
    );
    // Upper-case tags are looked at the same way, and a page with no head or a broken tag is an error and not a panic.
    assert!(!parts::preloads("<HEAD><LINK REL=\"preload\" HREF=\"./g_bg.wasm\"></HEAD>", &files).unwrap().to_lowercase().contains("g_bg"));
    assert!(parts::preloads("<body></body>", &files).unwrap_err().contains("no </head>"));
    assert!(parts::preloads("<head><link rel=\"preload\"", &files).unwrap_err().contains("unterminated"));
    assert!(parts::preloads("<head><linkage></head>", &files).is_ok(), "`<linkage` is no link");
}

// ------------------------------------------------------------------------------------------------------------------------------- what is made

/// The manifest of an assembled folder, as a JSON value.
fn manifest(out: &Dir) -> Value {
    serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap()).unwrap()
}

fn write_manifest(out: &Dir, value: &Value) {
    out.write("manifest.json", serde_json::to_string_pretty(value).unwrap());
}

#[test]
fn the_manifest_names_the_glue_the_parts_and_the_assets_with_their_sizes_and_hashes() {
    let bytes = module(&[(0, vec![1, b'x'])]);
    let out = out_dir_with(&bytes);
    let m = manifest(&out);
    assert_eq!(m["version"], 1);
    assert_eq!(m["glue"], glue_out());
    assert_eq!(m["module"]["bytes"], bytes.len());
    assert_eq!(m["module"]["sha256"], parts::sha256(&bytes));
    let listed = m["module"]["parts"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    let name = listed[0]["file"].as_str().unwrap();
    assert_eq!(name, format!("parts/diacraft-web-1-{}.wasm", &parts::sha256(&bytes)[..16]));
    assert_eq!(listed[0]["sha256"], parts::sha256(&bytes));
    assert_eq!(fs::read(out.path().join(name)).unwrap(), bytes);
    // The three assets, by the keys the program asks for.
    let assets = m["assets"].as_object().unwrap();
    assert_eq!(assets.keys().map(String::as_str).collect::<Vec<_>>(), ["cover", "licenses", "splash"]);
    for (key, _, stem, extension, _) in ASSETS {
        let entry = &assets[key];
        let file = entry["file"].as_str().unwrap();
        let on_disk = fs::read(out.path().join(file)).unwrap();
        assert_eq!(entry["bytes"], on_disk.len());
        assert_eq!(entry["sha256"], parts::sha256(&on_disk));
        assert_eq!(file, format!("assets/{stem}-{}.{extension}", &parts::sha256(&on_disk)[..16]));
    }
    assert_eq!(fs::read(out.path().join(assets["splash"]["file"].as_str().unwrap())).unwrap(), b"splash picture");
    assert_eq!(fs::read(out.path().join(assets["licenses"]["file"].as_str().unwrap())).unwrap(), b"DiaCraft: third-party licences (the web build)\n");
}

/// A module that is a header and one custom section of `payload` zero bytes: nothing but data, of the size a test needs.
fn module_of_size(payload: usize) -> Vec<u8> {
    let mut bytes = EMPTY_WASM.to_vec();
    bytes.push(0);
    // The section's size as LEB128: the name (one byte long, and one byte of length) and the zeros.
    let mut size = payload + 2;
    loop {
        let low = (size & 0x7f) as u8;
        size >>= 7;
        bytes.push(if size == 0 { low } else { low | 0x80 });
        if size == 0 {
            break;
        }
    }
    bytes.extend([1, b'x']);
    bytes.resize(bytes.len() + payload, 0);
    bytes
}

#[test]
fn a_module_of_several_parts_is_cut_joined_and_checked() {
    let bytes = module_of_size(PART_LIMIT as usize * 9 / 4);
    let out = out_dir_with(&bytes);
    let m = manifest(&out);
    let listed = m["module"]["parts"].as_array().unwrap();
    assert_eq!(listed.len(), 3);
    let sizes: Vec<u64> = listed.iter().map(|p| p["bytes"].as_u64().unwrap()).collect();
    assert!(sizes.iter().all(|s| *s <= PART_LIMIT) && sizes.iter().sum::<u64>() == bytes.len() as u64, "{sizes:?}");
    let joined: Vec<u8> = listed.iter().flat_map(|p| fs::read(out.path().join(p["file"].as_str().unwrap())).unwrap()).collect();
    assert_eq!(joined, bytes, "the parts, in order, are the module");
    let report = app::check(out.path()).unwrap();
    assert_eq!(report.module.parts, sizes);
    assert!(report.lines().iter().any(|l| l.contains("in 3 parts, none over 8.00 MiB")), "{:?}", report.lines());
}

#[test]
fn a_module_over_the_budget_is_refused_by_the_check_and_one_at_it_is_not() {
    let at = module_of_size(MODULE_BUDGET as usize - 15);
    assert_eq!(at.len() as u64, MODULE_BUDGET);
    app::check(out_dir_with(&at).path()).unwrap();
    let over = module_of_size(MODULE_BUDGET as usize - 14);
    assert_eq!(over.len() as u64, MODULE_BUDGET + 1);
    let error = app::check(out_dir_with(&over).path()).unwrap_err();
    assert!(error.contains("over the budget of") && error.contains("docs/website.md"), "{error}");
}

// ------------------------------------------------------------------------------------------------------------------------------ what is refused

/// A change to the finished folder, and the words the check fails with.
fn refuses(change: impl Fn(&Dir), diagnostic: &str) {
    let out = out_dir_with(&module(&[(0, vec![1, b'x'])]));
    change(&out);
    let error = app::check(out.path()).unwrap_err();
    assert!(error.contains(diagnostic), "expected {diagnostic:?} in {error:?}");
}

fn first_part(out: &Dir) -> String {
    manifest(out)["module"]["parts"][0]["file"].as_str().unwrap().to_owned()
}

fn asset_file(out: &Dir, key: &str) -> String {
    manifest(out)["assets"][key]["file"].as_str().unwrap().to_owned()
}

#[test]
fn a_part_that_is_missing_damaged_cut_short_or_renamed_is_refused() {
    // The page preloads every part, so a part that is not there is first the page's reference to a file the build did not make; the manifest says the same in its own words.
    refuses(|o| fs::remove_file(o.path().join(first_part(o))).unwrap(), "names a file the build did not make");
    refuses(
        |o| {
            let path = o.path().join(first_part(o));
            let mut bytes = fs::read(&path).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            fs::write(path, bytes).unwrap();
        },
        "do not have the hash",
    );
    refuses(
        |o| {
            let path = o.path().join(first_part(o));
            let bytes = fs::read(&path).unwrap();
            fs::write(path, &bytes[..bytes.len() - 1]).unwrap();
        },
        "bytes on disk",
    );
    // The name carries the hash: a file under another name is not the part.
    refuses(
        |o| {
            let name = first_part(o);
            fs::rename(o.path().join(&name), o.path().join("parts/diacraft-web-1-0000000000000000.wasm")).unwrap();
        },
        "names a file the build did not make",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["module"]["parts"][0]["file"] = json!("parts/diacraft-web-1-0000000000000000.wasm");
            write_manifest(o, &m);
        },
        "its name must be",
    );
    // The manifest naming a part that is not in the folder, with a page that preloads what the manifest says: the manifest's own words.
    refuses(
        |o| {
            let name = first_part(o);
            fs::remove_file(o.path().join(&name)).unwrap();
            o.write(
                "index.html",
                fs::read_to_string(o.path().join("index.html"))
                    .unwrap()
                    .replace(&format!("<link rel=\"preload\" href=\"./{name}\" as=\"fetch\" crossorigin=\"anonymous\">"), ""),
            );
        },
        "is not in the app's folder",
    );
    // A file the manifest does not name should not be there.
    refuses(|o| o.write("parts/diacraft-web-9-ffffffffffffffff.wasm", EMPTY_WASM), "does not name it");
    refuses(|o| o.write("assets/other.png", "x"), "does not name it");
}

#[test]
fn a_manifest_that_is_not_the_one_the_loader_reads_is_refused() {
    refuses(|o| o.write("manifest.json", "not json"), "not JSON");
    refuses(|o| o.write("manifest.json", "{}"), "not the version (1)");
    refuses(
        |o| {
            let mut m = manifest(o);
            m["version"] = json!(2);
            write_manifest(o, &m);
        },
        "not the version (1)",
    );
    for (value, diagnostic) in [
        (json!("missing.js"), "is not a JavaScript file of the app's folder"),
        (json!("../x.js"), "is not a JavaScript file of the app's folder"),
        (json!("start.js"), "is not a JavaScript file of the app's folder"),
        (json!("index.html"), "is not a JavaScript file of the app's folder"),
        (json!(7), "has no \"glue\""),
    ] {
        refuses(
            |o| {
                let mut m = manifest(o);
                m["glue"] = value.clone();
                write_manifest(o, &m);
            },
            diagnostic,
        );
    }
    // The module's own numbers.
    refuses(
        |o| {
            let mut m = manifest(o);
            m["module"]["bytes"] = json!(m["module"]["bytes"].as_u64().unwrap() + 1);
            write_manifest(o, &m);
        },
        "the parts add up to",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["module"]["sha256"] = json!("0".repeat(64));
            write_manifest(o, &m);
        },
        "not the module whose hash it gives",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["module"]["parts"] = json!([]);
            write_manifest(o, &m);
        },
        "between 1 and 16 are allowed",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["module"]["parts"][0]["sha256"] = json!("xyz");
            write_manifest(o, &m);
        },
        "not 64 hexadecimal digits",
    );
    // The manifest is read to a bound.
    refuses(|o| o.write("manifest.json", " ".repeat(parts::MAX_MANIFEST as usize + 1)), "at most");
    // And a vendor's name is no more in it than in the rest of the public text (the names come from the brands gate's list, so that this
    // file names none of them).
    for name in xtask::brands::DENY {
        refuses(
            |o| {
                let mut m = manifest(o);
                m["note"] = json!(name.to_uppercase());
                write_manifest(o, &m);
            },
            &format!("banned name {name}"),
        );
    }
}

#[test]
fn an_asset_that_is_missing_damaged_or_over_its_limit_is_refused() {
    refuses(|o| fs::remove_file(o.path().join(asset_file(o, "splash"))).unwrap(), "not in the app's folder");
    refuses(
        |o| {
            let path = o.path().join(asset_file(o, "cover"));
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 1;
            fs::write(path, bytes).unwrap();
        },
        "do not have the hash",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["assets"].as_object_mut().unwrap().remove("cover");
            write_manifest(o, &m);
        },
        "2 assets",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["assets"]["extra"] = m["assets"]["cover"].clone();
            m["assets"].as_object_mut().unwrap().remove("cover");
            write_manifest(o, &m);
        },
        "no asset \"cover\"",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["assets"]["splash"]["bytes"] = json!(0);
            write_manifest(o, &m);
        },
        "the program takes at most",
    );
    refuses(
        |o| {
            let mut m = manifest(o);
            m["assets"]["splash"]["file"] = json!("assets/diacraft-splash-0000000000000000.png");
            write_manifest(o, &m);
        },
        "its name must be",
    );
    // The licence list is read as text, and held to the same names as the rest of the site's text: the file as the repository has it, with a vendor's name in it, is refused.
    let root = repo();
    root.write("THIRD-PARTY-LICENSES-WEB.txt", "Copyright (c) Microsoft Corporation\n");
    let out = Dir::new("named");
    app::assemble(root.path(), &root.path().join("dist/web"), out.path(), &settings("https://app.example.invalid")).unwrap();
    assert!(app::check(out.path()).unwrap_err().contains("banned name microsoft"));
    // The copyright line of a licence that the repository's own check keeps as the licence has it is kept here too.
    let kept = xtask::brands::NAME_EXCEPTIONS.iter().find(|(file, _)| *file == "THIRD-PARTY-LICENSES-WEB.txt").map(|(_, line)| *line).unwrap();
    let root = repo();
    root.write("THIRD-PARTY-LICENSES-WEB.txt", format!("Copyright (c) 2026 {kept}\n"));
    let out = Dir::new("kept");
    app::assemble(root.path(), &root.path().join("dist/web"), out.path(), &settings("https://app.example.invalid")).unwrap();
    app::check(out.path()).unwrap();
    // ... and any other line with that name is not.
    let root = repo();
    let name = kept.split(' ').next().unwrap();
    root.write("THIRD-PARTY-LICENSES-WEB.txt", format!("Copyright (c) 2026 {kept}\nBy {name}\n"));
    let out = Dir::new("other");
    app::assemble(root.path(), &root.path().join("dist/web"), out.path(), &settings("https://app.example.invalid")).unwrap();
    assert!(app::check(out.path()).unwrap_err().contains(&format!("banned name {}", name.to_lowercase())));
}

#[test]
fn the_build_is_refused_when_what_the_repository_gives_it_is_missing_or_too_big() {
    for (file, diagnostic) in [
        ("apps/diacraft-web/loader.js", "loader.js"),
        ("apps/diacraft-web/_headers", "_headers"),
        ("assets/branding/diacraft-splash.png", "diacraft-splash.png"),
        ("assets/branding/diacraft-cover.png", "diacraft-cover.png"),
        ("THIRD-PARTY-LICENSES-WEB.txt", "THIRD-PARTY-LICENSES-WEB.txt"),
    ] {
        let root = repo();
        fs::remove_file(root.path().join(file)).unwrap();
        let out = Dir::new("missing");
        let error = app::assemble(root.path(), &root.path().join("dist/web"), out.path(), &settings("https://app.example.invalid")).unwrap_err();
        assert!(error.contains(diagnostic), "{file}: {error}");
    }
    // Data over what the program takes of it is not copied into the site: it would only be refused by the program, at run time, on every visit.
    for (file, limit) in [("assets/branding/diacraft-splash.png", 1u64 << 20), ("THIRD-PARTY-LICENSES-WEB.txt", 2 << 20)] {
        let root = repo();
        fs::File::create(root.path().join(file)).unwrap().set_len(limit + 1).unwrap();
        let out = Dir::new("big");
        let error = app::assemble(root.path(), &root.path().join("dist/web"), out.path(), &settings("https://app.example.invalid")).unwrap_err();
        assert!(error.contains("may be read"), "{file}: {error}");
    }
}

#[test]
fn the_pages_preloads_must_be_the_manifests_parts_and_nothing_else() {
    let page = |o: &Dir| fs::read_to_string(o.path().join("index.html")).unwrap();
    // A part left out: its fetch by the loader would not be answered from a preload, and the first bytes would wait for the loader to start.
    refuses(
        |o| {
            let name = first_part(o);
            o.write("index.html", page(o).replace(&format!("<link rel=\"preload\" href=\"./{name}\" as=\"fetch\" crossorigin=\"anonymous\">"), ""));
        },
        "the page preloads",
    );
    // Not `as="fetch"`, or no `crossorigin`: the loader's fetch is a CORS request, and would not be answered from such a preload, so the file would come twice.
    refuses(
        |o| {
            let name = first_part(o);
            o.write("index.html", page(o).replace(&format!("href=\"./{name}\" as=\"fetch\""), &format!("href=\"./{name}\" as=\"script\"")));
        },
        "that is not `as=\"fetch\"` with `crossorigin`",
    );
    refuses(
        |o| {
            o.write(
                "index.html",
                page(o).replace("href=\"./manifest.json\" as=\"fetch\" crossorigin=\"anonymous\"", "href=\"./manifest.json\" as=\"fetch\""),
            );
        },
        "that is not `as=\"fetch\"` with `crossorigin`",
    );
    // A preload of anything else is a file that comes whether or not anybody asks.
    refuses(
        |o| {
            o.write(
                "index.html",
                page(o).replace("</head>", "<link rel=\"preload\" href=\"./icon-192.png\" as=\"fetch\" crossorigin=\"anonymous\"></head>"),
            );
        },
        "the page preloads",
    );
}

#[test]
fn the_check_wants_the_hashed_files_cached_for_good_and_the_start_script_is_the_loader() {
    let out = out_dir_with(EMPTY_WASM);
    assert_eq!(fs::read_to_string(out.path().join("start.js")).unwrap(), LOADER);
    assert!(!fs::read_to_string(out.path().join("index.html")).unwrap().contains("import init"), "Trunk's inline script is not what the page holds");
    let no_cache = HEADERS.replace("/parts/*\n  Cache-Control: public, max-age=31536000, immutable\n\n", "");
    out.write("_headers", no_cache);
    assert!(app::check(out.path()).unwrap_err().contains("must be cached for good"));
}

/// The page is read in one pass: a page of two megabytes of tags is read in the time its size asks and not its size times its tags (a pass that lower-cased the whole page for each tag
/// would need minutes). The test is a guard against a hang, not a stopwatch: it fails only if the work is not done within a minute.
#[test]
fn a_page_full_of_tags_is_read_in_one_pass() {
    let tag = "<link rel=\"x\">";
    let page = format!("<head>{}</head>", tag.repeat((app::MAX_TEXT as usize - 64) / tag.len()));
    let (sender, receiver) = std::sync::mpsc::channel();
    let for_thread = page.clone();
    std::thread::spawn(move || {
        let made = parts::preloads(&for_thread, &["parts/diacraft-web-1-aa.wasm".to_owned()]).map(|made| made.len());
        let checked = parts::check_preloads_for_test(&for_thread, &[]);
        // `Err` only if the test has stopped waiting, which it has then failed.
        let _ = sender.send((made, checked.is_err()));
    });
    let (made, check_failed) = receiver.recv_timeout(std::time::Duration::from_secs(60)).expect("the page is read in one pass");
    assert!(made.unwrap() > page.len(), "the preload tags were added");
    assert!(check_failed, "a page that preloads nothing the manifest names is refused");
}
