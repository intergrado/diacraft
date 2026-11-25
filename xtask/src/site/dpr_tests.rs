//! Tests of `cargo xtask web-dpr` (`dpr.rs`): the verdict on a canvas, the scenarios, the file server, the browser's arguments, and what the command holds in common with the web app's source. The
//! run in a browser is not a test: it needs Chrome, and is what the command is for (`docs/website.md`). Tests may unwrap and panic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use super::dpr::{
    BROWSER_CANDIDATES, Browser, CANVAS_ID, LOADING_ID, Probe, Profile, SCENARIOS, Step, TOLERANCE, browser_args, content_type, create_private_dir,
    find_browser, judge, parse_args, parse_probe, probe_script, random_name, remove_folder, request_target, resolve, run, serve, started_script,
};

fn step(width: u32, height: u32, ratio: f64) -> Step {
    Step { width, height, ratio }
}

fn probe(ratio: f64, css: (f64, f64), canvas: (f64, f64)) -> Probe {
    Probe { ratio, css, canvas }
}

/// A folder of its own under the system's temporary folder, removed when it is dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("xtask-dpr-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------- the verdict

/// The bug that was reported, as the browser showed it: a 1200 by 700 page at a ratio of 2 with a canvas of 1200 by 700 pixels. The words say what it means for the window.
#[test]
fn a_canvas_that_is_the_pages_css_size_at_a_ratio_of_two_fails() {
    let error = judge(&step(1200, 700, 2.0), &probe(2.0, (1200.0, 700.0), (1200.0, 700.0))).unwrap_err();
    assert!(error.contains("1200x700 pixels") && error.contains("need 2400x1400"), "{error}");
    assert!(error.contains("laid out for 600x350 points, not the page's 1200x700"), "{error}");
}

#[test]
fn the_ratios_of_the_report_fail_with_their_own_numbers() {
    for (ratio, points) in [(1.5, "800x467"), (3.0, "400x233")] {
        let error = judge(&step(1200, 700, ratio), &probe(ratio, (1200.0, 700.0), (1200.0, 700.0))).unwrap_err();
        assert!(error.contains(&format!("laid out for {points} points")), "{error}");
    }
    let phone = judge(&step(390, 844, 3.0), &probe(3.0, (390.0, 844.0), (390.0, 844.0))).unwrap_err();
    assert!(phone.contains("laid out for 130x281 points, not the page's 390x844"), "{phone}");
}

#[test]
fn a_canvas_of_the_pages_size_in_device_pixels_passes() {
    let line = judge(&step(1200, 700, 2.0), &probe(2.0, (1200.0, 700.0), (2400.0, 1400.0))).unwrap();
    assert!(line.contains("2400x1400 pixels") && line.contains("1200x700 points"), "{line}");
    assert!(judge(&step(390, 844, 3.0), &probe(3.0, (390.0, 844.0), (1170.0, 2532.0))).is_ok());
    assert!(judge(&step(1200, 700, 1.0), &probe(1.0, (1200.0, 700.0), (1200.0, 700.0))).is_ok());
    // The size a real Chrome gave at a real ratio of 1.5, with a fractional height.
    assert!(judge(&step(1200, 613, 1.5), &probe(1.5, (1200.0, 613.328125), (1800.0, 920.0))).is_ok());
}

#[test]
fn the_browsers_rounding_passes_and_one_pixel_more_does_not() {
    assert_eq!(TOLERANCE, 3);
    let page = step(1200, 700, 2.0);
    assert!(judge(&page, &probe(2.0, (1200.0, 700.0), (2403.0, 1397.0))).is_ok());
    assert!(judge(&page, &probe(2.0, (1200.0, 700.0), (2404.0, 1400.0))).is_err());
    assert!(judge(&page, &probe(2.0, (1200.0, 700.0), (2400.0, 1396.0))).is_err());
}

/// A canvas that is right for a page that is not the one that was emulated proves nothing: the check says the emulation did not take.
#[test]
fn an_emulation_that_did_not_take_is_not_a_pass() {
    let ratio = judge(&step(1200, 700, 2.0), &probe(1.0, (1200.0, 700.0), (1200.0, 700.0))).unwrap_err();
    assert!(ratio.contains("device pixel ratio is 1, not the 2"), "{ratio}");
    let size = judge(&step(1200, 700, 2.0), &probe(2.0, (600.0, 350.0), (1200.0, 700.0))).unwrap_err();
    assert!(size.contains("does not fill the page"), "{size}");
}

#[test]
fn the_page_answer_is_read_and_a_bad_one_is_refused() {
    let text = r#"{"ratio":2,"css":[1200,700.5],"canvas":[2400,1401]}"#;
    assert_eq!(parse_probe(text).unwrap(), probe(2.0, (1200.0, 700.5), (2400.0, 1401.0)));
    for bad in [
        "",
        "null",
        "[]",
        r#"{"ratio":2}"#,
        r#"{"ratio":2,"css":[1],"canvas":[1,2]}"#,
        r#"{"ratio":"2","css":[1,2],"canvas":[1,2]}"#,
        r#"{"ratio":null,"css":[1,2],"canvas":[1,2]}"#,
    ] {
        assert!(parse_probe(bad).is_err(), "{bad:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------- the scenarios

#[test]
fn the_scenarios_cover_the_ratios_of_the_report_a_phone_and_a_resize() {
    let first_steps: Vec<(u32, u32, f64, bool)> =
        SCENARIOS.iter().filter_map(|s| s.steps.first().map(|st| (st.width, st.height, st.ratio, s.mobile))).collect();
    for ratio in [1.0, 1.5, 2.0, 3.0] {
        assert!(first_steps.iter().any(|&(w, h, r, mobile)| (w, h) == (1200, 700) && r == ratio && !mobile), "no desktop scenario at {ratio}");
    }
    assert!(first_steps.contains(&(390, 844, 3.0, true)), "no phone scenario");
    let resize = SCENARIOS.iter().find(|s| s.steps.len() > 1).expect("a scenario that resizes the running app");
    let ratios: Vec<f64> = resize.steps.iter().map(|s| s.ratio).collect();
    assert!(ratios.windows(2).all(|w| w[0] != w[1]), "each step of the resize changes the ratio: {ratios:?}");
    assert!(resize.steps.windows(2).all(|w| (w[0].width, w[0].height) != (w[1].width, w[1].height)), "each step changes the size");
    let mut names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SCENARIOS.len(), "scenario names are distinct");
}

// ----------------------------------------------------------------------------------------------------------------- what it shares with the app

fn app_source(rel: &str) -> String {
    fs::read_to_string(xtask::root().join("apps/diacraft-web").join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The check lets go what the app's own guard lets go: a check stricter than the guard would fail a canvas the app accepts, and a laxer one would pass a canvas the guard corrects.
#[test]
fn the_tolerance_is_the_guards() {
    let source = app_source("src/canvas_size.rs");
    assert!(source.contains(&format!("pub const TOLERANCE: u32 = {TOLERANCE};")), "src/canvas_size.rs has another TOLERANCE than {TOLERANCE}");
}

/// The scripts read the elements the page has: the ids are the program's (`page.rs`) and the page's (`index.html`).
#[test]
fn the_scripts_read_the_elements_of_the_page() {
    let page = app_source("src/page.rs");
    assert!(page.contains(&format!("pub const CANVAS_ID: &str = \"{CANVAS_ID}\";")));
    assert!(page.contains(&format!("pub const LOADING_ID: &str = \"{LOADING_ID}\";")));
    let html = app_source("index.html");
    assert!(html.contains(&format!("id=\"{CANVAS_ID}\"")) && html.contains(&format!("id=\"{LOADING_ID}\"")));
    assert!(probe_script().contains(CANVAS_ID) && started_script().contains(CANVAS_ID) && started_script().contains(LOADING_ID));
}

// ------------------------------------------------------------------------------------------------------------------------------ the file server

#[test]
fn a_request_is_read_for_its_path() {
    assert_eq!(request_target("GET / HTTP/1.1\r\nHost: x\r\n\r\n"), Some((true, "/".to_owned())));
    assert_eq!(request_target("GET /parts/a.wasm?x=1#y HTTP/1.1\r\n\r\n"), Some((true, "/parts/a.wasm".to_owned())));
    assert_eq!(request_target("HEAD /a HTTP/1.1\r\n\r\n"), Some((false, "/a".to_owned())));
    for bad in ["POST /a HTTP/1.1\r\n\r\n", "GET a HTTP/1.1\r\n\r\n", "GET", "", "\r\n"] {
        assert_eq!(request_target(bad), None, "{bad:?}");
    }
}

#[test]
fn only_a_regular_file_of_the_folder_is_served() {
    let folder = Folder::new("resolve");
    let root = &folder.0;
    fs::write(root.join("index.html"), "<html>").unwrap();
    fs::create_dir_all(root.join("parts")).unwrap();
    fs::write(root.join("parts").join("a.wasm"), [0, 97, 115, 109]).unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();
    assert_eq!(resolve(root, "/"), Some(root.join("index.html")));
    assert_eq!(resolve(root, "/index.html"), Some(root.join("index.html")));
    assert_eq!(resolve(root, "/parts/a.wasm"), Some(root.join("parts").join("a.wasm")));
    assert_eq!(resolve(root, "//parts//a.wasm"), Some(root.join("parts").join("a.wasm")));
    for bad in [
        "/missing",
        "/parts",
        "/parts/",
        "/empty/",
        "/../index.html",
        "/parts/../../x",
        "/./index.html",
        "/a\\b",
        "/C:/x",
        "/%2e%2e/x",
        "/a\u{0}b",
        "/a\nb",
    ] {
        assert_eq!(resolve(root, bad), None, "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_link_is_not_followed() {
    let folder = Folder::new("link");
    fs::write(folder.0.join("outside.txt"), "secret").unwrap();
    let inner = folder.0.join("app");
    fs::create_dir_all(&inner).unwrap();
    std::os::unix::fs::symlink(folder.0.join("outside.txt"), inner.join("link.txt")).unwrap();
    assert_eq!(resolve(&inner, "/link.txt"), None);
}

#[test]
fn a_wasm_file_is_told_to_be_wasm() {
    for (name, kind) in [
        ("a.wasm", "application/wasm"),
        ("a.js", "text/javascript; charset=utf-8"),
        ("index.html", "text/html; charset=utf-8"),
        ("page.css", "text/css; charset=utf-8"),
        ("manifest.json", "application/json"),
        ("d.webmanifest", "application/manifest+json"),
        ("i.png", "image/png"),
        ("noextension", "application/octet-stream"),
    ] {
        assert_eq!(content_type(Path::new(name)), kind, "{name}");
    }
}

/// What a plain client gets from the server: the status line, the head and the body.
fn fetch(port: u16, request: &str) -> (String, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut all = Vec::new();
    stream.read_to_end(&mut all).unwrap();
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").expect("a head");
    let head = String::from_utf8(all[..split].to_vec()).unwrap();
    let status = head.lines().next().unwrap().to_owned();
    (status, head, all[split + 4..].to_vec())
}

#[test]
fn the_server_answers_files_and_refuses_the_rest() {
    let folder = Folder::new("serve");
    fs::write(folder.0.join("index.html"), "<html>hi</html>").unwrap();
    fs::write(folder.0.join("a.wasm"), [0, 97, 115, 109]).unwrap();
    let port = serve(folder.0.clone()).unwrap();
    let (status, head, body) = fetch(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(head.contains("Content-Type: text/html; charset=utf-8") && head.contains("Content-Length: 15"), "{head}");
    assert_eq!(body, b"<html>hi</html>");
    let (_, head, body) = fetch(port, "GET /a.wasm HTTP/1.1\r\n\r\n");
    assert!(head.contains("Content-Type: application/wasm"), "{head}");
    assert_eq!(body, [0, 97, 115, 109]);
    let (status, _, _) = fetch(port, "GET /missing.js HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 404 Not Found");
    let (status, _, _) = fetch(port, "GET /../a.wasm HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 404 Not Found");
    let (status, _, _) = fetch(port, "POST / HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 405 Method Not Allowed");
    // A head has the length and no body.
    let (status, head, body) = fetch(port, "HEAD /a.wasm HTTP/1.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(head.contains("Content-Length: 4"), "{head}");
    assert!(body.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------------------- the browser

#[test]
fn the_browser_starts_headless_with_a_profile_of_its_own_and_software_graphics() {
    let args = browser_args(Path::new("/profiles/p"), "");
    for needed in [
        "--headless=new",
        "--remote-debugging-port=0",
        "--user-data-dir=/profiles/p",
        "--use-angle=swiftshader",
        "--enable-unsafe-swiftshader",
        "--no-first-run",
    ] {
        assert!(args.iter().any(|a| a == needed), "no {needed} in {args:?}");
    }
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    // The user's arguments come before the page, and an empty one adds nothing.
    let more = browser_args(Path::new("/p"), "  --no-sandbox   --lang=en ");
    let at = |what: &str| more.iter().position(|a| a == what).unwrap();
    assert!(at("--no-sandbox") < at("about:blank") && at("--lang=en") < at("about:blank"));
    assert!(!more.iter().any(|a| a.is_empty()));
}

#[test]
fn a_browser_that_is_named_and_does_not_run_is_an_error_that_names_it() {
    let error = find_browser(Some("no-such-browser-for-diacraft")).unwrap_err();
    assert!(error.contains("CHROME names `no-such-browser-for-diacraft`"), "{error}");
    assert!(BROWSER_CANDIDATES.contains(&"google-chrome") && BROWSER_CANDIDATES.contains(&"chromium"));
}

#[cfg(unix)]
#[test]
fn a_named_program_that_runs_is_the_browser() {
    assert_eq!(find_browser(Some("true")).unwrap(), "true");
}

// ---------------------------------------------------------------------------------------------------------------------------- the command

#[test]
fn the_folder_is_dist_app_unless_one_is_named() {
    let root = Path::new("/repo");
    assert_eq!(parse_args(root, &[]).unwrap(), Path::new("/repo/dist/app"));
    assert_eq!(parse_args(root, &["out/app"]).unwrap(), Path::new("out/app"));
    for bad in [&["a", "b"][..], &["--help"], &[""]] {
        assert!(parse_args(root, bad).unwrap_err().starts_with("usage: cargo xtask web-dpr"), "{bad:?}");
    }
}

#[test]
fn a_folder_that_is_not_the_app_is_refused_before_a_browser_is_started() {
    let folder = Folder::new("notapp");
    let error = run(Path::new("/repo"), &[folder.0.to_str().unwrap()]).unwrap_err();
    assert!(error.contains("has no index.html") && error.contains("cargo xtask site --web"), "{error}");
    fs::write(folder.0.join("index.html"), "<html>").unwrap();
    let error = run(Path::new("/repo"), &[folder.0.to_str().unwrap()]).unwrap_err();
    assert!(error.contains("has no manifest.json"), "{error}");
}

// -------------------------------------------------------------------------------------------------------------------------- the profile folder

#[test]
fn a_profile_name_is_long_random_and_not_made_from_the_process() {
    let names: std::collections::HashSet<String> = (0..200).map(|_| random_name()).collect();
    assert_eq!(names.len(), 200, "two names were the same");
    for name in &names {
        let hex = name.strip_prefix("diacraft-web-dpr-").expect("the prefix");
        assert_eq!(hex.len(), 32, "{name}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
        assert_ne!(*name, format!("diacraft-web-dpr-{}", std::process::id()), "a name another process can work out");
    }
}

/// A folder that is there is someone's: it is refused, with what is in it as it was, and so are a file of the name and a missing parent.
#[test]
fn a_folder_that_is_there_is_refused_and_not_reused() {
    let base = Folder::new("exists");
    let taken = base.0.join("taken");
    fs::create_dir_all(&taken).unwrap();
    fs::write(taken.join("keep"), "x").unwrap();
    assert_eq!(create_private_dir(&taken).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    assert!(taken.join("keep").is_file());
    fs::write(base.0.join("file"), "x").unwrap();
    assert!(create_private_dir(&base.0.join("file")).is_err());
    assert!(create_private_dir(&base.0.join("no").join("child")).is_err());
    assert!(!base.0.join("no").exists(), "the parents are not made");
    create_private_dir(&base.0.join("new")).unwrap();
    assert!(base.0.join("new").is_dir());
}

#[cfg(unix)]
#[test]
fn the_profile_folder_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt as _;
    let base = Folder::new("mode");
    let profile = Profile::create_with(&base.0, random_name).unwrap();
    let mode = fs::metadata(profile.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "{mode:o}");
}

#[test]
fn a_taken_name_is_skipped_and_its_folder_is_left_alone() {
    let base = Folder::new("taken");
    fs::create_dir_all(base.0.join("a")).unwrap();
    fs::write(base.0.join("a").join("DevToolsActivePort"), "1\n/devtools/browser/x\n").unwrap();
    let mut names = vec!["c", "b", "a"];
    let profile = Profile::create_with(&base.0, || names.pop().unwrap_or("z").to_owned()).unwrap();
    assert_eq!(profile.path(), base.0.join("b"));
    assert!(profile.path().read_dir().unwrap().next().is_none(), "a new profile starts empty");
    drop(profile);
    assert!(!base.0.join("b").exists(), "the profile is removed when it is dropped");
    assert!(base.0.join("a").join("DevToolsActivePort").is_file(), "the folder that was there is not touched");
}

#[test]
fn names_that_are_all_taken_are_an_error() {
    let base = Folder::new("alltaken");
    fs::create_dir_all(base.0.join("same")).unwrap();
    let error = Profile::create_with(&base.0, || "same".to_owned()).unwrap_err();
    assert!(error.contains("16 tries"), "{error}");
}

/// The case the security scan raised: a `DevToolsActivePort` that was on the machine before the browser started. The profile used to be `<temporary folder>/diacraft-web-dpr-<process number>`, made with `create_dir_all` (which
/// takes a folder that is there), so a file planted at that path was read as the browser's and the check would have driven whatever listened on the port in it. A program that ends at once (`true`) writes no file of its own, so
/// anything the start returns was not written by it.
#[cfg(unix)]
#[test]
fn a_port_file_planted_where_the_old_name_was_is_not_taken_for_the_browsers() {
    let base = Folder::new("planted");
    let planted = base.0.join(format!("diacraft-web-dpr-{}", std::process::id()));
    fs::create_dir_all(&planted).unwrap();
    fs::write(planted.join("DevToolsActivePort"), "9\n/devtools/browser/planted\n").unwrap();
    let error = Browser::start_in(&base.0, "true", "").err().expect("a program that ends at once is no browser");
    assert!(error.contains("ended at once"), "{error}");
    assert!(planted.join("DevToolsActivePort").is_file(), "the folder that was there is not touched");
    let left: Vec<_> = fs::read_dir(&base.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left.len(), 1, "the profile of the failed start is removed: {left:?}");
}

#[cfg(unix)]
#[test]
fn a_program_that_cannot_start_leaves_no_profile_behind() {
    let base = Folder::new("nostart");
    let error = Browser::start_in(&base.0, "no-such-browser-for-diacraft", "").err().expect("no such program");
    assert!(error.contains("could not start"), "{error}");
    assert_eq!(fs::read_dir(&base.0).unwrap().count(), 0, "the profile folder is removed");
}

/// A browser's own processes are still writing in its profile when it has just been stopped, and the first removals fail: the folder is removed again until it goes, and a folder that is gone counts as removed.
#[test]
fn a_folder_that_will_not_go_at_once_is_removed_again() {
    let mut calls = 0;
    let result = remove_folder(Path::new("/p"), 5, std::time::Duration::ZERO, |_| {
        calls += 1;
        if calls < 3 { Err(std::io::Error::other("still in use")) } else { Ok(()) }
    });
    assert!(result.is_ok());
    assert_eq!(calls, 3, "it stops at the first removal that works");
    let mut calls = 0;
    assert!(
        remove_folder(Path::new("/p"), 5, std::time::Duration::ZERO, |_| {
            calls += 1;
            Err(std::io::ErrorKind::NotFound.into())
        })
        .is_ok()
    );
    assert_eq!(calls, 1, "a folder that is gone is removed");
    let mut calls = 0;
    let error = remove_folder(Path::new("/p"), 4, std::time::Duration::ZERO, |_| {
        calls += 1;
        Err(std::io::Error::other("busy"))
    })
    .unwrap_err();
    assert_eq!((calls, error.to_string().as_str()), (4, "busy"), "it gives up after the tries, with the last failure");
}
