//! Tests of the check after a deploy (`live.rs`): what it finds to fetch, what it says of each response, how it reads what `curl` prints, and, once, `curl` itself against a
//! small server of the test's own.

use super::live::{self, Curl, Fetch, Level, Report, Response};
use std::cell::RefCell;
use std::collections::BTreeMap;

// ------------------------------------------------------------------------------------------------------------------------------ a site that is

struct Served {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// A site in memory: what is served at each path, and what was asked.
#[derive(Default)]
struct Fake {
    files: BTreeMap<String, Served>,
    asked: RefCell<Vec<String>>,
    /// Paths whose request fails, as a network failure does.
    broken: Vec<String>,
    /// The address of the site itself (`https://host`), when the stand-in knows it.
    origin: Option<String>,
}

const REVALIDATE: &str = "public, max-age=0, must-revalidate";
const KEEP: &str = "public, max-age=31536000, immutable";

impl Fake {
    /// A file with `Cache-Control` and `Content-Encoding` as the arguments say, and an `ETag` of its own.
    fn put(&mut self, path: &str, body: &str, cache: &str, encoding: Option<&str>) -> &mut Self {
        let mut headers = vec![("cache-control".to_owned(), cache.to_owned()), ("etag".to_owned(), format!("\"{}\"", path.len() * 7))];
        if let Some(encoding) = encoding {
            headers.push(("content-encoding".to_owned(), encoding.to_owned()));
        }
        self.files.insert(path.to_owned(), Served { status: 200, headers, body: body.as_bytes().to_vec() });
        self
    }

    fn header(&mut self, path: &str, name: &str, value: Option<&str>) {
        let served = self.files.get_mut(path).unwrap();
        served.headers.retain(|(n, _)| n != name);
        if let Some(value) = value {
            served.headers.push((name.to_owned(), value.to_owned()));
        }
    }
}

impl Fetch for Fake {
    fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    fn get(&self, path: &str, if_none_match: Option<&str>, _body: bool) -> Result<Response, String> {
        self.asked.borrow_mut().push(format!("{path}{}", if if_none_match.is_some() { " (if-none-match)" } else { "" }));
        if self.broken.iter().any(|p| p == path) {
            return Err("could not connect".into());
        }
        let Some(served) = self.files.get(path) else {
            return Ok(Response { status: 404, headers: vec![("cache-control".into(), REVALIDATE.into())], size: 9, body: b"not found".to_vec() });
        };
        let etag = served.headers.iter().find(|(n, _)| n == "etag").map(|(_, v)| v.as_str());
        if served.status == 200 && if_none_match.is_some() && if_none_match == etag {
            return Ok(Response { status: 304, headers: served.headers.clone(), size: 0, body: Vec::new() });
        }
        Ok(Response { status: served.status, headers: served.headers.clone(), size: served.body.len() as u64, body: served.body.clone() })
    }
}

/// A deployed site with a folder of an app in it, as the check finds its files (the one check serves both of the two sites, which have no file in common: this fixture holds what each has, in one):
/// pages and a style that are asked about, pictures and the app's files that are named by their hash and kept.
fn site() -> Fake {
    let mut site = Fake::default();
    let long = "x".repeat(2000);
    site.put(
        "/",
        &format!(
            "<html><head><link rel=\"stylesheet\" href=\"/styles.css\"><link rel=\"manifest\" href=\"/site.webmanifest\"><link rel=\"canonical\" href=\"https://example.invalid/\">\
             <meta property=\"og:image\" content=\"https://example.invalid/assets/social.jpg\"></head><body><picture><source srcset=\"/assets/hashed/octopus.4c9f9c16802f.webp 1x, /assets/hashed/big.0123456789ab.webp 2x\">\
             <img src=\"/assets/hashed/octopus.4c9f9c16802f.webp\"></picture><a href=\"/app/\">app</a><a href=\"/privacy.html\">p</a><!-- {long} --></body></html>"
        ),
        REVALIDATE,
        Some("br"),
    );
    site.put("/privacy.html", &format!("<html><body><img src=\"/assets/hashed/guide.aaaaaaaaaaaa.webp\"><a href=\"/\">home</a><a href=\"/download.zip\">download</a><!-- {long} --></body></html>"), REVALIDATE, Some("br"));
    site.put("/assets/hashed/guide.aaaaaaaaaaaa.webp", "image", KEEP, None);
    site.put("/styles.css", &format!("body {{ background: url('/assets/paper.png') }} /* {long} */"), REVALIDATE, Some("br"));
    site.put("/site.webmanifest", r#"{"icons":[{"src":"/assets/icon-192.png"}]}"#, REVALIDATE, None);
    for plain in ["/assets/paper.png", "/assets/icon-192.png", "/assets/social.jpg"] {
        site.put(plain, "image", REVALIDATE, None);
    }
    for hashed in ["/assets/hashed/octopus.4c9f9c16802f.webp", "/assets/hashed/big.0123456789ab.webp"] {
        site.put(hashed, "image", KEEP, None);
    }
    site.put(
        "/app/",
        &format!(
            "<html><head><link rel=\"icon\" href=\"./diacraft-icon-256-0123456789abcdef.png\"><link rel=\"manifest\" href=\"diacraft.webmanifest\">\
             <link rel=\"stylesheet\" href=\"./page-0123456789abcdef.css\"><script type=\"module\" src=\"./start.js\"></script>\
             <link rel=\"modulepreload\" href=\"./diacraft-web-0123456789abcdef.js\"><link rel=\"modulepreload\" href=\"./snippets/eframe-1/inline0.js\">\
             <link rel=\"preload\" href=\"./manifest.json\" as=\"fetch\"><link rel=\"preload\" href=\"./parts/diacraft-web-1-0123456789abcdef.wasm\" as=\"fetch\"></head><!-- {long} --></html>"
        ),
        REVALIDATE,
        Some("br"),
    );
    site.put("/app/diacraft.webmanifest", r#"{"icons":[{"src":"icon-192.png"},{"src":"icon-512.png"}]}"#, REVALIDATE, None);
    site.put(
        "/app/manifest.json",
        r#"{"version":1,"glue":"diacraft-web-0123456789abcdef.js","module":{"parts":[{"file":"parts/diacraft-web-1-0123456789abcdef.wasm"},{"file":"parts/diacraft-web-2-fedcba9876543210.wasm"}]},"assets":{"cover":{"file":"assets/diacraft-cover-0123456789abcdef.png"},"splash":{"file":"assets/diacraft-splash-0123456789abcdef.png"}}}"#,
        REVALIDATE,
        Some("br"),
    );
    for plain in ["/app/start.js", "/app/icon-192.png", "/app/icon-512.png", "/app/snippets/eframe-1/inline0.js"] {
        site.put(plain, "plain", REVALIDATE, None);
    }
    site.put("/app/start.js", &long, REVALIDATE, Some("br"));
    site.put("/app/snippets/eframe-1/inline0.js", &long, REVALIDATE, Some("br"));
    site.put("/app/diacraft-web-0123456789abcdef.js", &long, KEEP, Some("br"));
    site.put("/app/page-0123456789abcdef.css", &long, KEEP, Some("br"));
    site.put("/app/diacraft-icon-256-0123456789abcdef.png", "icon", KEEP, None);
    site.put("/app/parts/diacraft-web-1-0123456789abcdef.wasm", &long, KEEP, Some("br"));
    site.put("/app/parts/diacraft-web-2-fedcba9876543210.wasm", &long, KEEP, Some("br"));
    site.put("/app/assets/diacraft-cover-0123456789abcdef.png", "cover", KEEP, None);
    site.put("/app/assets/diacraft-splash-0123456789abcdef.png", "splash", KEEP, None);
    site
}

/// The web app at the root of a site of its own: the page is `/`, and its files are the folder's. The pages link another site: the address of the pages.
fn app_site() -> Fake {
    let mut site = Fake::default();
    let long = "x".repeat(2000);
    site.put(
        "/",
        &format!(
            "<html><head><link rel=\"icon\" href=\"./diacraft-icon-256-0123456789abcdef.png\"><link rel=\"manifest\" href=\"diacraft.webmanifest\">\
             <link rel=\"stylesheet\" href=\"./page-0123456789abcdef.css\"><script type=\"module\" src=\"./start.js\"></script>\
             <link rel=\"modulepreload\" href=\"./diacraft-web-0123456789abcdef.js\"><link rel=\"modulepreload\" href=\"./snippets/eframe-1/inline0.js\">\
             <link rel=\"preload\" href=\"./manifest.json\" as=\"fetch\"><link rel=\"preload\" href=\"./parts/diacraft-web-1-0123456789abcdef.wasm\" as=\"fetch\"></head>\
             <body><a href=\"https://pages.example.invalid/privacy.html\">privacy</a><!-- {long} --></body></html>"
        ),
        REVALIDATE,
        Some("br"),
    );
    site.put("/diacraft.webmanifest", r#"{"icons":[{"src":"icon-192.png"},{"src":"icon-512.png"}]}"#, REVALIDATE, None);
    site.put(
        "/manifest.json",
        r#"{"version":1,"glue":"diacraft-web-0123456789abcdef.js","module":{"parts":[{"file":"parts/diacraft-web-1-0123456789abcdef.wasm"},{"file":"parts/diacraft-web-2-fedcba9876543210.wasm"}]},"assets":{"cover":{"file":"assets/diacraft-cover-0123456789abcdef.png"},"splash":{"file":"assets/diacraft-splash-0123456789abcdef.png"}}}"#,
        REVALIDATE,
        Some("br"),
    );
    for plain in ["/icon-192.png", "/icon-512.png"] {
        site.put(plain, "plain", REVALIDATE, None);
    }
    for text in ["/start.js", "/snippets/eframe-1/inline0.js"] {
        site.put(text, &long, REVALIDATE, Some("br"));
    }
    site.put("/diacraft-web-0123456789abcdef.js", &long, KEEP, Some("br"));
    site.put("/page-0123456789abcdef.css", &long, KEEP, Some("br"));
    site.put("/diacraft-icon-256-0123456789abcdef.png", "icon", KEEP, None);
    site.put("/parts/diacraft-web-1-0123456789abcdef.wasm", &long, KEEP, Some("br"));
    site.put("/parts/diacraft-web-2-fedcba9876543210.wasm", &long, KEEP, Some("br"));
    site.put("/assets/diacraft-cover-0123456789abcdef.png", "cover", KEEP, None);
    site.put("/assets/diacraft-splash-0123456789abcdef.png", "splash", KEEP, None);
    site
}

fn paths(report: &Report) -> Vec<&str> {
    report.entries.iter().map(|e| e.path.as_str()).collect()
}

fn entry<'a>(report: &'a Report, path: &str) -> &'a live::Entry {
    report.entries.iter().find(|e| e.path == path).unwrap_or_else(|| panic!("{path} was not probed: {:?}", paths(report)))
}

fn said(report: &Report, path: &str, level: Level) -> Vec<String> {
    entry(report, path).findings.iter().filter(|f| f.level == level).map(|f| f.text.clone()).collect()
}

// ----------------------------------------------------------------------------------------------------------------------------------- the pass

#[test]
fn a_site_as_deployed_passes_and_every_file_it_loads_is_found() {
    let fake = site();
    let report = live::check(&fake).unwrap();
    assert_eq!(report.failures(), 0, "{:#?}", report.lines());
    assert_eq!(report.warnings(), 0, "{:#?}", report.lines());
    let mut expected = vec![
        "/",
        "/app/",
        "/app/assets/diacraft-cover-0123456789abcdef.png",
        "/app/assets/diacraft-splash-0123456789abcdef.png",
        "/app/diacraft-icon-256-0123456789abcdef.png",
        "/app/diacraft-web-0123456789abcdef.js",
        "/app/diacraft.webmanifest",
        "/app/icon-192.png",
        "/app/icon-512.png",
        "/app/manifest.json",
        "/app/page-0123456789abcdef.css",
        "/app/parts/diacraft-web-1-0123456789abcdef.wasm",
        "/app/parts/diacraft-web-2-fedcba9876543210.wasm",
        "/app/snippets/eframe-1/inline0.js",
        "/app/start.js",
        "/assets/hashed/big.0123456789ab.webp",
        "/assets/hashed/guide.aaaaaaaaaaaa.webp",
        "/assets/hashed/octopus.4c9f9c16802f.webp",
        "/assets/icon-192.png",
        "/assets/paper.png",
        "/assets/social.jpg",
        "/privacy.html",
        "/site.webmanifest",
        "/styles.css",
    ];
    expected.sort_unstable();
    let mut found = paths(&report);
    found.sort_unstable();
    assert_eq!(
        found, expected,
        "the files of the page, of the pages it links to, of the app's page, of the module's manifest and of the web manifests, once each"
    );
    // Each says what it was sent, and each that has an ETag was asked again with it.
    let part = entry(&report, "/app/parts/diacraft-web-1-0123456789abcdef.wasm");
    assert_eq!((part.status, part.cache_control.as_deref(), part.encoding.as_deref()), (200, Some(KEEP), Some("br")));
    assert!(part.etag.is_some() && part.conditional == Some(304) && part.size == 2000);
    assert_eq!(entry(&report, "/").conditional, Some(304));
    // A link to a page is followed, a link to a download is not, and the canonical address is no file.
    assert!(!paths(&report).iter().any(|p| p.contains("download.zip") || p.contains("canonical")));
}

/// The app is a site of its own: its start page is `/`, every file it loads is found from there, and a link to the other site (the pages) is not followed, since that
/// site has its own check.
#[test]
fn the_app_at_the_root_of_its_own_site_passes_and_every_file_it_loads_is_found() {
    let fake = app_site();
    let report = live::check(&fake).unwrap();
    assert_eq!(report.failures(), 0, "{:#?}", report.lines());
    assert_eq!(report.warnings(), 0, "{:#?}", report.lines());
    let mut expected = vec![
        "/",
        "/assets/diacraft-cover-0123456789abcdef.png",
        "/assets/diacraft-splash-0123456789abcdef.png",
        "/diacraft-icon-256-0123456789abcdef.png",
        "/diacraft-web-0123456789abcdef.js",
        "/diacraft.webmanifest",
        "/icon-192.png",
        "/icon-512.png",
        "/manifest.json",
        "/page-0123456789abcdef.css",
        "/parts/diacraft-web-1-0123456789abcdef.wasm",
        "/parts/diacraft-web-2-fedcba9876543210.wasm",
        "/snippets/eframe-1/inline0.js",
        "/start.js",
    ];
    expected.sort_unstable();
    let mut found = paths(&report);
    found.sort_unstable();
    assert_eq!(found, expected, "the start page, the files it loads, and from the module's manifest the parts and the data, once each");
    assert!(!fake.asked.borrow().iter().any(|a| a.contains("privacy")), "a link to the pages is not followed");
    // The same faults as before are faults at the root: a part that is not kept, and a loader that is.
    let mut fake = app_site();
    fake.put("/parts/diacraft-web-1-0123456789abcdef.wasm", "x", REVALIDATE, None);
    fake.put("/start.js", "x", KEEP, None);
    let report = live::check(&fake).unwrap();
    assert_eq!(said(&report, "/parts/diacraft-web-1-0123456789abcdef.wasm", Level::Fail).len(), 1);
    assert_eq!(said(&report, "/start.js", Level::Fail).len(), 1);
    // A site with no start page cannot be looked at.
    let mut fake = app_site();
    fake.files.remove("/");
    let error = live::check(&fake).unwrap_err();
    assert!(error.contains("start page") && error.contains("404"), "{error}");
}

#[test]
fn the_requests_ask_for_the_compressed_forms_and_the_second_carries_the_etag() {
    let fake = site();
    live::check(&fake).unwrap();
    let asked = fake.asked.borrow();
    let part = "/app/parts/diacraft-web-1-0123456789abcdef.wasm";
    assert_eq!(asked.iter().filter(|a| a.as_str() == part).count(), 1);
    assert_eq!(asked.iter().filter(|a| a.as_str() == format!("{part} (if-none-match)")).count(), 1);
}

#[test]
fn the_lines_have_one_row_for_each_file_and_a_count() {
    let report = live::check(&site()).unwrap();
    let lines = report.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("/app/parts/diacraft-web-1-0123456789abcdef.wasm") && l.contains("br") && l.contains("304") && l.starts_with("ok")),
        "{lines:#?}"
    );
    assert!(
        lines.last().unwrap().contains("24 URLs") && lines.last().unwrap().contains("0 failures") && lines.last().unwrap().contains("0 warnings"),
        "{lines:#?}"
    );
}

// ------------------------------------------------------------------------------------------------------------------------------ what fails

#[test]
fn a_file_named_by_its_hash_that_is_not_kept_for_a_year_fails() {
    for weak in [REVALIDATE, "public, max-age=3600", "public, max-age=31536000", "immutable"] {
        let mut fake = site();
        fake.put("/app/diacraft-icon-256-0123456789abcdef.png", "icon", weak, None);
        let report = live::check(&fake).unwrap();
        let path = "/app/diacraft-icon-256-0123456789abcdef.png";
        let failures = said(&report, path, Level::Fail);
        assert!(failures.len() == 1 && failures[0].contains(weak) && failures[0].contains("immutable"), "{weak}: {failures:?}");
        assert_eq!(report.failures(), 1);
    }
    // A hashed name with no Cache-Control at all.
    let mut fake = site();
    fake.header("/assets/hashed/big.0123456789ab.webp", "cache-control", None);
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/assets/hashed/big.0123456789ab.webp", Level::Fail)[0].contains("no Cache-Control"));
}

#[test]
fn a_file_that_is_not_named_by_a_hash_and_is_kept_fails() {
    for kept in [KEEP, "public, max-age=3600", "s-maxage=60", "immutable"] {
        let mut fake = site();
        fake.put("/app/start.js", "x", kept, None);
        let report = live::check(&fake).unwrap();
        let failures = said(&report, "/app/start.js", Level::Fail);
        assert!(failures.len() == 1 && failures[0].contains(kept) && failures[0].contains("not named by a hash"), "{kept}: {failures:?}");
    }
    // Asked about on every visit, in any of the ways: fine. Nothing said: a warning that the browser would guess.
    for fine in ["public, max-age=0, must-revalidate", "no-cache", "no-store", "max-age=0"] {
        let mut fake = site();
        fake.put("/app/start.js", "x", fine, None);
        let report = live::check(&fake).unwrap();
        assert!(said(&report, "/app/start.js", Level::Fail).is_empty(), "{fine}");
        assert!(said(&report, "/app/start.js", Level::Warn).is_empty(), "{fine}");
    }
    let mut fake = site();
    fake.header("/app/start.js", "cache-control", None);
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/app/start.js", Level::Fail).is_empty());
    assert!(said(&report, "/app/start.js", Level::Warn).iter().any(|w| w.contains("no Cache-Control")));
    let mut fake = site();
    fake.put("/app/start.js", "x", "public", None);
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/app/start.js", Level::Warn).iter().any(|w| w.contains("does not say to ask again")));
}

#[test]
fn a_file_that_is_not_there_fails_and_the_rest_are_still_probed() {
    let mut fake = site();
    fake.files.remove("/app/parts/diacraft-web-2-fedcba9876543210.wasm");
    let report = live::check(&fake).unwrap();
    let failures = said(&report, "/app/parts/diacraft-web-2-fedcba9876543210.wasm", Level::Fail);
    assert!(failures.len() == 1 && failures[0].contains("404"), "{failures:?}");
    assert!(paths(&report).contains(&"/app/start.js"), "what comes after is probed");
    assert_eq!(report.failures(), 1);
}

#[test]
fn a_request_that_fails_is_a_failure_of_its_file_and_the_home_page_that_cannot_be_had_stops_the_check() {
    let mut fake = site();
    fake.broken.push("/app/start.js".into());
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/app/start.js", Level::Fail)[0].contains("could not connect"));
    // Nothing can be found without the home page.
    let mut fake = site();
    fake.broken.push("/".into());
    assert!(live::check(&fake).unwrap_err().contains("could not connect"));
    let mut fake = site();
    fake.files.remove("/");
    let error = live::check(&fake).unwrap_err();
    assert!(error.contains("/") && error.contains("404"), "{error}");
}

#[test]
fn the_apps_page_that_is_missing_is_a_failure_and_the_site_is_still_checked() {
    let mut fake = site();
    fake.files.remove("/app/");
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/app/", Level::Fail)[0].contains("404"));
    assert!(paths(&report).contains(&"/styles.css"));
}

// ----------------------------------------------------------------------------------------------------------------------------- what warns

#[test]
fn a_wasm_part_that_comes_back_uncompressed_is_a_warning_and_not_a_failure() {
    let mut fake = site();
    let long = "x".repeat(2000);
    fake.put("/app/parts/diacraft-web-1-0123456789abcdef.wasm", &long, KEEP, None);
    let report = live::check(&fake).unwrap();
    let warnings = said(&report, "/app/parts/diacraft-web-1-0123456789abcdef.wasm", Level::Warn);
    assert!(warnings.len() == 1 && warnings[0].contains(".wasm") && warnings[0].contains("uncompressed"), "{warnings:?}");
    assert_eq!(report.failures(), 0);
    assert_eq!(report.warnings(), 1);
    // Compressed with gzip is compressed; a small file is not worth it.
    fake.put("/app/parts/diacraft-web-1-0123456789abcdef.wasm", &long, KEEP, Some("gzip"));
    assert_eq!(live::check(&fake).unwrap().warnings(), 0);
    fake.put("/app/parts/diacraft-web-1-0123456789abcdef.wasm", "tiny", KEEP, None);
    assert_eq!(live::check(&fake).unwrap().warnings(), 0);
    // The loader and the style are text: they warn too, and pictures never do.
    fake.put("/app/start.js", &long, REVALIDATE, None);
    fake.put("/assets/paper.png", &long, REVALIDATE, None);
    let report = live::check(&fake).unwrap();
    assert_eq!(said(&report, "/app/start.js", Level::Warn).len(), 1);
    assert!(said(&report, "/assets/paper.png", Level::Warn).is_empty());
}

#[test]
fn a_file_with_no_etag_or_no_304_is_a_warning() {
    let mut fake = site();
    fake.header("/styles.css", "etag", None);
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/styles.css", Level::Warn).iter().any(|w| w.contains("no ETag")));
    assert_eq!(entry(&report, "/styles.css").conditional, None);
    // A server that answers 200 to a conditional request: the second request is made, and what it got is said.
    struct NoConditional(Fake);
    impl Fetch for NoConditional {
        fn get(&self, path: &str, _if_none_match: Option<&str>, body: bool) -> Result<Response, String> {
            self.0.get(path, None, body)
        }
    }
    let report = live::check(&NoConditional(site())).unwrap();
    let warnings = said(&report, "/styles.css", Level::Warn);
    assert!(warnings.iter().any(|w| w.contains("200") && w.contains("304")), "{warnings:?}");
    assert_eq!(entry(&report, "/styles.css").conditional, Some(200));
    assert_eq!(report.failures(), 0);
}

#[test]
fn more_files_than_the_check_looks_at_are_an_error() {
    let mut fake = site();
    let images: String = (0..300).map(|n| format!("<img src=\"/assets/many{n}.png\">")).collect();
    fake.put("/", &format!("<html><body>{images}</body></html>"), REVALIDATE, None);
    let error = live::check(&fake).unwrap_err();
    assert!(error.contains("more than 200"), "{error}");
}

// ---------------------------------------------------------------------------------------------------------------------- what a page refers to

#[test]
fn the_references_of_a_page_are_what_it_loads_and_nothing_it_only_links() {
    let html = r##"<html><head>
        <link rel="stylesheet" href="a.css"><link rel="icon" href="/b.png"><link rel="apple-touch-icon" href="c.png"><link rel="manifest" href="m.webmanifest">
        <link rel="preload" href="./d.json" as="fetch"><link rel="modulepreload" href="e.js"><link rel="canonical" href="https://example.invalid/x"><link rel="alternate" href="/feed.xml">
        <script src="f.js"></script><script>inline()</script><meta property="og:image" content="https://example.invalid/g.jpg"><meta name="description" content="not.a.file">
        </head><body><img src="h.png" srcset="h1.png 1x, h2.png 2x"><picture><source srcset="i.webp 480w, /j.webp 960w"></picture>
        <a href="/page.html">a page</a><img src="data:image/png;base64,AAAA"><img src="#frag"><img src="https://other.invalid/k.png"></body></html>"##;
    let mut found = live::html_refs(html);
    found.sort();
    assert_eq!(
        found,
        [
            "./d.json",
            "/b.png",
            "/j.webp",
            "a.css",
            "c.png",
            "e.js",
            "f.js",
            "h.png",
            "h1.png",
            "h2.png",
            "https://example.invalid/g.jpg",
            "i.webp",
            "m.webmanifest"
        ]
    );
}

#[test]
fn the_links_of_a_page_are_found_to_follow_the_ones_to_the_site_itself() {
    let html = r##"<a href="/a.html">a</a><A HREF="b/">b</A><a href="https://github.com/x">x</a><a href="//cdn.invalid/y">y</a><a name="n">no href</a><abbr href="z">z</abbr><a href="#top">top</a>"##;
    let mut links = live::html_links(html);
    links.sort();
    assert_eq!(links, ["#top", "/a.html", "b/"]);
}

#[test]
fn a_page_that_a_page_links_to_and_that_is_not_there_fails() {
    let mut fake = site();
    fake.files.remove("/privacy.html");
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/privacy.html", Level::Fail)[0].contains("404"));
}

#[test]
fn a_reference_becomes_a_path_of_the_site_or_nothing() {
    let at = |page: &str, reference: &str| live::resolve_ref(page, reference);
    assert_eq!(at("/", "styles.css").as_deref(), Some("/styles.css"));
    assert_eq!(at("/", "/assets/a.png").as_deref(), Some("/assets/a.png"));
    assert_eq!(at("/app/", "./start.js").as_deref(), Some("/app/start.js"));
    assert_eq!(at("/app/", "diacraft.webmanifest").as_deref(), Some("/app/diacraft.webmanifest"));
    assert_eq!(at("/app/", "../assets/a.png").as_deref(), Some("/assets/a.png"));
    assert_eq!(at("/app/index.html", "x.js?v=2#top").as_deref(), Some("/app/x.js"));
    assert_eq!(at("/app/", "parts/./a.wasm").as_deref(), Some("/app/parts/a.wasm"));
    assert_eq!(at("/", "../../a.png").as_deref(), Some("/a.png"), "it cannot go above the root");
    // The address of the site's own canonical domain is the path on the deployment being checked.
    assert_eq!(at("/", "https://example.invalid/assets/social.jpg").as_deref(), Some("/assets/social.jpg"));
    assert_eq!(at("/", "//example.invalid/a.png").as_deref(), Some("/a.png"));
    // Not files: data, fragments, a page alone, an empty reference, scripts, a link with a space in it.
    for no in ["data:image/png;base64,AAAA", "#top", "", "javascript:x()", "mailto:a@b.invalid", "a b.png", "/a\\b.png", "https://example.invalid"] {
        assert_eq!(at("/", no), None, "{no}");
    }
    // A folder is the folder's page.
    assert_eq!(at("/", "guides/").as_deref(), Some("/guides/"));
}

// --------------------------------------------------------------------------------------------------------------------------- reading curl

#[test]
fn what_curl_prints_is_a_head_a_body_and_the_size_it_counted() {
    let raw = b"HTTP/2 200 \r\ncache-control: public, max-age=0, must-revalidate\r\nContent-Encoding: br\r\nETag: W/\"abc\"\r\nx-twice: 1\r\nx-twice: 2\r\n\r\n\x00\x01body\r\n\r\nbytes\n@@size=1234\n";
    let response = live::parse_response(raw).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.header("cache-control"), Some("public, max-age=0, must-revalidate"));
    assert_eq!(response.header("content-encoding"), Some("br"));
    assert_eq!(response.header("etag"), Some("W/\"abc\""));
    assert_eq!(response.header("x-twice"), Some("1"));
    assert_eq!(response.header("missing"), None);
    assert_eq!(response.size, 1234);
    assert_eq!(response.body, b"\x00\x01body\r\n\r\nbytes");
    // Informational heads before the answer, LF-only line ends, a 304 with no body.
    let raw = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 304 Not Modified\nETag: \"x\"\n\n\n@@size=0\n";
    let response = live::parse_response(raw).unwrap();
    assert_eq!((response.status, response.header("etag"), response.size, response.body.len()), (304, Some("\"x\""), 0, 0));
}

#[test]
fn output_that_is_not_a_response_is_an_error_and_not_a_panic() {
    for raw in [
        &b""[..],
        b"@@size=1\n",
        b"HTTP/2 200\r\ncache-control: x\r\n\r\nbody",
        b"HTTP/2 20x \r\n\r\n\n@@size=1\n",
        b"HTTP/2\r\n\r\n\n@@size=1\n",
        b"not http\r\n\r\n\n@@size=1\n",
        b"HTTP/2 200 \r\nno colon here\r\n\r\n\n@@size=1\n",
        b"HTTP/2 200 \r\n\r\n\n@@size=notanumber\n",
        b"HTTP/2 200 \r\n\r\n\n@@size=99999999999999999999999999\n",
        b"HTTP/2 200 \r\nunfinished",
        b"HTTP/2 100 \r\n\r\n\n@@size=1\n",
    ] {
        assert!(live::parse_response(raw).is_err(), "{:?}", String::from_utf8_lossy(raw));
    }
    // Every prefix of a good one is an answer or an error.
    let good = b"HTTP/2 200 \r\netag: \"a\"\r\n\r\nbody\n@@size=4\n";
    for cut in 0..good.len() {
        let _ = live::parse_response(good.get(..cut).unwrap());
    }
}

#[test]
fn the_address_is_a_site_with_a_scheme_and_a_host_and_nothing_that_could_be_an_argument() {
    assert_eq!(live::parse_base("https://example.invalid").unwrap(), "https://example.invalid");
    assert_eq!(live::parse_base("https://example.invalid/").unwrap(), "https://example.invalid");
    assert_eq!(live::parse_base("http://127.0.0.1:8099").unwrap(), "http://127.0.0.1:8099");
    assert_eq!(live::parse_base("https://diacraft.pages.dev").unwrap(), "https://diacraft.pages.dev");
    for bad in [
        "",
        "example.invalid",
        "ftp://example.invalid",
        "https://",
        "https:///x",
        "https://example.invalid/path",
        "https://example.invalid?x=1",
        "https://example.invalid#frag",
        "https://user@example.invalid",
        "https://exa mple.invalid",
        "https://example.invalid\n",
        "https://example.invalid'",
        "--url=https://example.invalid",
        "-o/etc/passwd",
    ] {
        assert!(live::parse_base(bad).is_err(), "{bad:?}");
    }
    assert!(live::parse_base(&format!("https://{}", "a".repeat(300))).is_err());
}

#[test]
fn the_command_takes_the_address_of_each_site_and_says_how_when_it_does_not() {
    assert!(live::run(&[]).unwrap_err().contains("usage: cargo xtask deploy-check <BASE_URL> [<BASE_URL>]"));
    assert!(live::run(&["a", "b"]).unwrap_err().contains("usage"));
    assert!(live::run(&["not a url"]).unwrap_err().contains("usage: cargo xtask deploy-check"));
    // Three are one too many, and one address twice is not two sites.
    assert!(live::run(&["https://a.example", "https://b.example", "https://c.example"]).unwrap_err().contains("usage: cargo xtask deploy-check"));
    let error = live::run(&["https://a.example", "https://a.example/"]).unwrap_err();
    assert!(error.contains("both addresses are https://a.example") && error.contains("two sites"), "{error}");
    // Either of the two that is not an address is the one that is named.
    assert!(live::run(&["https://a.example", "b.example"]).unwrap_err().contains("`b.example` is not an address"));
}

// ------------------------------------------------------------------------------------------------------------------------ curl, for real

/// A server of the test's own that answers the next `count` connections: a page with an ETag that answers 304 to its own, and a 404 for anything else.
fn serve(count: usize) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (mut etag_given, mut encoding) = (false, String::new());
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap() == 0 || header.trim().is_empty() {
                    break;
                }
                let lower = header.to_ascii_lowercase();
                etag_given |= lower.starts_with("if-none-match:") && lower.contains("\"v1\"");
                if lower.starts_with("accept-encoding:") {
                    encoding = header.trim().to_owned();
                }
            }
            requests.push(format!("{path} {} {}", if etag_given { "conditional" } else { "plain" }, encoding));
            let body = "<html><body><img src=\"/a.png\"></body></html>";
            let response = match (path.as_str(), etag_given) {
                ("/", true) => {
                    "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nCache-Control: public, max-age=0, must-revalidate\r\nConnection: close\r\n\r\n"
                        .to_owned()
                }
                ("/", false) => format!(
                    "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nCache-Control: public, max-age=0, must-revalidate\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
                _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (port, handle)
}

#[test]
fn curl_makes_the_requests_the_check_wants_and_its_output_is_read() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl is not installed: the one test that runs it is skipped");
        return;
    }
    let (port, server) = serve(3);
    let curl = Curl::new(&format!("http://127.0.0.1:{port}")).unwrap();
    // The first request asks for the compressed forms and does not decode them; the second carries the ETag; the third asks for the body, decoded.
    let first = curl.get("/", None, false).unwrap();
    assert_eq!(
        (first.status, first.header("etag"), first.header("cache-control")),
        (200, Some("\"v1\""), Some("public, max-age=0, must-revalidate"))
    );
    assert_eq!(first.size, 44);
    assert!(first.body.is_empty(), "the body of a request that wants none is not kept");
    let second = curl.get("/", Some("\"v1\""), false).unwrap();
    assert_eq!((second.status, second.size), (304, 0));
    let third = curl.get("/missing", None, true).unwrap();
    assert_eq!(third.status, 404);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("/ plain") && requests[0].to_ascii_lowercase().contains("accept-encoding: br, gzip"), "{requests:?}");
    assert!(requests[1].starts_with("/ conditional"), "{requests:?}");
    // The body of a request that wants it is decoded by curl and read.
    let (port, server) = serve(1);
    let curl = Curl::new(&format!("http://127.0.0.1:{port}")).unwrap();
    let page = curl.get("/", None, true).unwrap();
    assert_eq!(page.body, b"<html><body><img src=\"/a.png\"></body></html>");
    server.join().unwrap();
}

#[test]
fn curl_that_cannot_connect_is_an_error_with_what_it_said() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    // A port nobody listens on: bind and drop.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let curl = Curl::new(&format!("http://127.0.0.1:{port}")).unwrap();
    let error = curl.get("/", None, false).unwrap_err();
    assert!(error.contains("curl") && !error.is_empty(), "{error}");
}

/// The command, with curl, for each of the two sites: each is checked on its own, from its start page, and the run fails if either has a fault.
#[test]
fn the_command_checks_each_site_it_is_given_with_curl() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    // A site asks its server for `/` (with its body), `/` and `/` with its ETag, and `/a.png`, which the server has not: four connections, and a failure of that picture.
    let (first, one) = serve(4);
    let (second, two) = serve(4);
    let error = live::run(&[&format!("http://127.0.0.1:{first}"), &format!("http://127.0.0.1:{second}")]).unwrap_err();
    assert!(error.contains("2 files failed"), "each site has the picture it does not serve: {error}");
    // A server that was not asked as often as it was told to wait is let go, so that a fault of the command is a failure of this test and not a hang.
    for (port, handle) in [(first, &one), (second, &two)] {
        for _ in 0..20 {
            if handle.is_finished() {
                break;
            }
            // A connection that ends at once: the server answers it, and counts it.
            drop(std::net::TcpStream::connect(("127.0.0.1", port)));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let (one, two) = (one.join(), two.join());
    for requests in [one.unwrap(), two.unwrap()] {
        assert_eq!(requests.iter().filter(|r| r.starts_with("/ ")).count(), 3, "{requests:?}");
        assert_eq!(requests.iter().filter(|r| r.starts_with("/a.png ")).count(), 1, "{requests:?}");
    }
}

// -------------------------------------------------------------------------------------------------------- the 308 of Pages

/// What Cloudflare Pages does with a link to `from`, a page with its extension: a 308 to the same address without it.
fn redirect(site: &mut Fake, from: &str, to: &str) {
    site.put(from, "", REVALIDATE, None);
    let served = site.files.get_mut(from).unwrap();
    served.status = 308;
    served.headers.push(("location".to_owned(), to.to_owned()));
}

/// The pages as Pages serves them: `/privacy.html` is a 308 to `/privacy`, which has the page.
fn pages_with_redirect() -> Fake {
    let mut site = site();
    let page = String::from_utf8(site.files.get("/privacy.html").unwrap().body.clone()).unwrap();
    redirect(&mut site, "/privacy.html", "/privacy");
    site.put("/privacy", &page, REVALIDATE, Some("br"));
    site
}

#[test]
fn a_308_from_a_page_to_its_address_without_the_extension_is_what_pages_does_and_the_target_is_checked_as_a_file() {
    let fake = pages_with_redirect();
    let report = live::check(&fake).unwrap();
    assert_eq!(report.failures(), 0, "{:#?}", report.lines());
    assert_eq!(report.warnings(), 0, "{:#?}", report.lines());
    // The page with its extension answered 308 and is said to go to the other; the other is a file like any, asked for and asked again with its ETag.
    let page = entry(&report, "/privacy.html");
    assert_eq!((page.status, page.redirect.as_deref()), (308, Some("/privacy")));
    let target = entry(&report, "/privacy");
    assert_eq!((target.status, target.conditional), (200, Some(304)));
    assert!(report.lines().iter().any(|l| l.contains("/privacy.html -> /privacy")), "{:#?}", report.lines());
    // What the target page loads is found too, as a page's pictures are.
    assert!(paths(&report).contains(&"/assets/hashed/guide.aaaaaaaaaaaa.webp"), "{:?}", paths(&report));
    // The target comes right after the page that was sent to it, and once.
    let order = paths(&report);
    let at = order.iter().position(|p| *p == "/privacy.html").unwrap();
    assert_eq!(order[at + 1], "/privacy");
    assert_eq!(order.iter().filter(|p| **p == "/privacy").count(), 1);
}

#[test]
fn the_target_of_a_308_is_checked_as_any_file_is() {
    // A target that is not there, and one that a browser would keep for a year though it is no file named by its hash: each a failure of the target.
    let mut fake = pages_with_redirect();
    fake.files.remove("/privacy");
    let report = live::check(&fake).unwrap();
    assert_eq!(said(&report, "/privacy", Level::Fail).len(), 1, "{:#?}", report.lines());
    assert!(said(&report, "/privacy", Level::Fail)[0].contains("answered 404"));
    assert!(said(&report, "/privacy.html", Level::Fail).is_empty(), "the redirect itself is right");
    let mut fake = pages_with_redirect();
    fake.header("/privacy", "cache-control", Some(KEEP));
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/privacy", Level::Fail)[0].contains("would hide the next release"), "{:#?}", report.lines());
    // A target that sends on again is not followed twice: it is an answer that is not 200.
    let mut fake = pages_with_redirect();
    redirect(&mut fake, "/privacy", "/privacy/");
    let report = live::check(&fake).unwrap();
    assert!(said(&report, "/privacy", Level::Fail)[0].contains("answered 308"), "{:#?}", report.lines());
    assert!(!paths(&report).contains(&"/privacy/"), "{:?}", paths(&report));
}

#[test]
fn a_308_that_goes_anywhere_but_to_the_same_address_without_the_extension_is_a_failure() {
    for (location, why) in [
        (Some("/other"), "to `/other`"),
        (Some("/privacy.html"), "to `/privacy.html`"),
        (Some("/privacy?x=1"), "to `/privacy?x=1`"),
        (Some("/privacy#top"), "to `/privacy#top`"),
        (Some("//evil.example/privacy"), "to `//evil.example/privacy`"),
        (Some("https://evil.example/privacy"), "to `https://evil.example/privacy`"),
        (None, "no Location"),
    ] {
        let mut fake = pages_with_redirect();
        fake.files.get_mut("/privacy.html").unwrap().headers.retain(|(n, _)| n != "location");
        if let Some(location) = location {
            fake.header("/privacy.html", "location", Some(location));
        }
        let report = live::check(&fake).unwrap();
        let failures = said(&report, "/privacy.html", Level::Fail);
        assert_eq!(failures.len(), 1, "{location:?}: {:#?}", report.lines());
        assert!(failures[0].contains("answered 308") && failures[0].contains(why), "{location:?}: {failures:?}");
        assert!(entry(&report, "/privacy.html").redirect.is_none(), "{location:?}: not followed");
    }
    // Other redirects, and a 308 for a path that is not a page, are answers that are not 200, as they were.
    for (status, path) in [(301, "/privacy.html"), (302, "/privacy.html"), (307, "/privacy.html"), (308, "/styles.css"), (308, "/assets/paper.png")] {
        let mut fake = pages_with_redirect();
        if path != "/privacy.html" {
            redirect(&mut fake, path, &path.replace(".css", ""));
        }
        fake.files.get_mut(path).unwrap().status = status;
        let report = live::check(&fake).unwrap();
        let failures = said(&report, path, Level::Fail);
        assert_eq!(failures.len(), 1, "{status} {path}: {:#?}", report.lines());
        assert!(failures[0].contains(&format!("answered {status}, and a file of the site is answered 200")), "{failures:?}");
    }
}

#[test]
fn a_location_with_the_address_of_the_site_itself_is_the_same_as_a_path_and_one_with_another_host_is_not() {
    let mut fake = pages_with_redirect();
    fake.origin = Some("https://example.invalid".to_owned());
    fake.header("/privacy.html", "location", Some("https://example.invalid/privacy"));
    let report = live::check(&fake).unwrap();
    assert_eq!(report.failures(), 0, "{:#?}", report.lines());
    assert_eq!(entry(&report, "/privacy.html").redirect.as_deref(), Some("/privacy"));
    // The same path on another host, one that only starts like the site's, and another scheme.
    for location in [
        "https://evil.example/privacy",
        "https://example.invalid.evil.example/privacy",
        "http://example.invalid/privacy",
        "https://example.invalid:8443/privacy",
    ] {
        fake.header("/privacy.html", "location", Some(location));
        let report = live::check(&fake).unwrap();
        assert_eq!(said(&report, "/privacy.html", Level::Fail).len(), 1, "{location}: {:#?}", report.lines());
    }
    // A check that does not know its site's address (a stand-in) takes paths only.
    let mut fake = pages_with_redirect();
    fake.header("/privacy.html", "location", Some("https://example.invalid/privacy"));
    assert_eq!(said(&live::check(&fake).unwrap(), "/privacy.html", Level::Fail).len(), 1);
}

#[test]
fn an_index_page_is_sent_to_its_folder_and_the_start_page_is_probed_once() {
    // `/index.html` goes to `/`, which is the start page and is probed once; `/guides/index.html` to `/guides/` or `/guides`.
    let mut fake = site();
    fake.put("/guides/", "<html><body><!-- x --></body></html>", REVALIDATE, None);
    redirect(&mut fake, "/index.html", "/");
    redirect(&mut fake, "/guides/index.html", "/guides/");
    let home = String::from_utf8(fake.files.get("/").unwrap().body.clone()).unwrap();
    fake.put("/", &home.replace("</body>", "<a href=\"/index.html\">h</a><a href=\"/guides/index.html\">g</a></body>"), REVALIDATE, Some("br"));
    let report = live::check(&fake).unwrap();
    assert_eq!(report.failures(), 0, "{:#?}", report.lines());
    assert_eq!(entry(&report, "/index.html").redirect.as_deref(), Some("/"));
    assert_eq!(entry(&report, "/guides/index.html").redirect.as_deref(), Some("/guides/"));
    assert_eq!(paths(&report).iter().filter(|p| **p == "/").count(), 1, "{:?}", paths(&report));
    fake.header("/guides/index.html", "location", Some("/guides"));
    fake.put("/guides", "<html><body><!-- x --></body></html>", REVALIDATE, None);
    assert_eq!(live::check(&fake).unwrap().failures(), 0, "the folder with no slash is the same folder");
    // A page with an extension that is itself a 200 is as it was, and so is the 308 of a path that has no extension to take away.
    let fake = site();
    let report = live::check(&fake).unwrap();
    assert_eq!(entry(&report, "/privacy.html").redirect, None);
}

/// A server of the test's own that behaves as Pages does for one page: `/` links `/p.html`, which it answers with a 308 to `location` (`{port}` in it is the server's own), and `/p` is the page,
/// with an ETag that it answers 304 to. It serves until `stop` is set and returns the paths asked for.
fn serve_a_308(location: &'static str) -> (u16, std::sync::Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        while !flag.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let mut conditional = false;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap() == 0 || header.trim().is_empty() {
                    break;
                }
                conditional |= header.to_ascii_lowercase().starts_with("if-none-match:");
            }
            requests.push(path.clone());
            let cache = "Cache-Control: public, max-age=0, must-revalidate";
            let response = match (path.as_str(), conditional) {
                ("/", false) => format!(
                    "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\n{cache}\r\nContent-Length: 29\r\nConnection: close\r\n\r\n<a href=\"/p.html\">the page</a>"
                ),
                ("/p.html", _) => format!(
                    "HTTP/1.1 308 Permanent Redirect\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    location.replace("{port}", &port.to_string())
                ),
                ("/p", false) => {
                    format!("HTTP/1.1 200 OK\r\nETag: \"v1\"\r\n{cache}\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<html></html>")
                }
                ("/", true) | ("/p", true) => format!("HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\n{cache}\r\nConnection: close\r\n\r\n"),
                _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (port, stop, handle)
}

#[test]
fn the_command_follows_the_308_pages_sends_a_page_with_its_extension_through_curl() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    // The path, and the address of the site itself: both are the site's own, and the page is found and checked at the address it is sent to.
    for location in ["/p", "http://127.0.0.1:{port}/p"] {
        let (port, stop, server) = serve_a_308(location);
        let result = live::run(&[&format!("http://127.0.0.1:{port}")]);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let requests = server.join().unwrap();
        assert!(result.is_ok(), "{location}: {result:?}");
        assert!(requests.iter().any(|r| r == "/p"), "{location}: the target was asked for: {requests:?}");
    }
    // A 308 to another site is a failure, and the other site is not asked.
    let (port, stop, server) = serve_a_308("http://localhost.invalid/p");
    let result = live::run(&[&format!("http://127.0.0.1:{port}")]);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let requests = server.join().unwrap();
    assert!(result.unwrap_err().contains("1 file"), "a failure");
    assert!(!requests.iter().any(|r| r == "/p"), "{requests:?}");
}
