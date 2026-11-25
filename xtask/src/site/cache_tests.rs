//! Tests of what the browser is told to keep (`cache.rs`): the reading of `_headers` the way Pages applies it, the shape of a hashed name, and each rule of the check with a
//! crafted `_headers` that breaks it.

use super::cache::{self, Headers, Kind, Pattern, Summary, YEAR};
use super::parts::sha256;
use std::collections::BTreeMap;

fn matches(pattern: &str, path: &str) -> bool {
    Pattern::compile(pattern).unwrap().matches(path)
}

// --------------------------------------------------------------------------------------------------------------------------- patterns

#[test]
fn a_splat_matches_any_characters_slashes_too_and_it_may_be_in_the_middle_of_a_name() {
    assert!(matches("/app/*", "/app/index.html"));
    assert!(matches("/app/*", "/app/parts/a.wasm"));
    assert!(matches("/app/*", "/app/"));
    assert!(!matches("/app/*", "/app"));
    assert!(!matches("/app/*", "/other/app/x"));
    assert!(matches("/*", "/"));
    assert!(matches("/*", "/a/b/c.js"));
    // In the middle of a name: the rule for Trunk's files at the app's root.
    assert!(matches("/app/page-*.css", "/app/page-0123456789abcdef.css"));
    assert!(matches("/app/page-*.css", "/app/page-.css"));
    assert!(!matches("/app/page-*.css", "/app/page.css"));
    assert!(!matches("/app/page-*.css", "/app/page-x.css.map"));
    assert!(!matches("/app/page-*.css", "/app/other-x.css"));
    // A splat takes slashes with it, as Pages says ("greedily match all characters").
    assert!(matches("/app/page-*.css", "/app/page-x/y.css"));
    // Dots and other characters of a pattern are text and not regular expressions.
    assert!(!matches("/app/a.js", "/app/aXjs"));
    assert!(matches("/app/a.js", "/app/a.js"));
    assert!(!matches("/app/a+.js", "/app/aa.js"));
    assert!(matches("/app/a(1).js", "/app/a(1).js"));
}

#[test]
fn a_placeholder_matches_one_or_more_characters_but_a_slash() {
    assert!(matches("/movies/:title", "/movies/dune"));
    assert!(!matches("/movies/:title", "/movies/"));
    assert!(!matches("/movies/:title", "/movies/a/b"));
    assert!(!matches("/movies/:title", "/movies"));
    // In the middle of a segment, up to the text that follows.
    assert!(matches("/app/page-:hash.css", "/app/page-abc.css"));
    assert!(!matches("/app/page-:hash.css", "/app/page-.css"));
    assert!(!matches("/app/page-:hash.css", "/app/page-a/b.css"));
    assert!(matches("/:a/:b", "/x/y"));
    assert!(!matches("/:a/:b", "/x/y/z"));
    // A colon not followed by a letter is text.
    assert!(matches("/a:1", "/a:1"));
    assert!(matches("/a:", "/a:"));
    // A placeholder and a splat together.
    assert!(matches("/:dir/*", "/app/x/y"));
    assert!(!matches("/:dir/*", "//x"));
}

#[test]
fn a_pattern_pages_would_not_read_as_one_is_an_error() {
    assert!(Pattern::compile("/a/*/b/*").unwrap_err().contains("one splat"));
    assert!(Pattern::compile("app/*").unwrap_err().contains("start with /"));
    assert!(Pattern::compile("").unwrap_err().contains("start with /"));
    assert!(Pattern::compile("https://example.invalid/*").unwrap_err().contains("host"));
    assert!(Pattern::compile("/:a/:a").unwrap_err().contains("twice"));
    assert!(Pattern::compile("/:splat").unwrap_err().contains("splat"));
    let long: String = (0..40).map(|n| format!("/:a{n}")).collect();
    assert!(Pattern::compile(&long).unwrap_err().contains("pieces"));
}

#[test]
fn a_path_longer_than_the_check_reads_matches_nothing_and_a_hostile_path_is_not_a_hang() {
    let long = format!("/app/{}", "a".repeat(5000));
    assert!(!matches("/app/*", &long));
    // Many placeholders over a long run of text: the work is bounded by the pieces and the path.
    let pattern = "/:a-:b-:c-:d-:e-:f-:g-:h/*";
    let hostile = format!("/{}", "x-".repeat(400));
    let _ = matches(pattern, &hostile);
    assert!(matches(pattern, "/1-2-3-4-5-6-7-8/z"));
}

// ------------------------------------------------------------------------------------------------------------------------- the file

const SITE_HEADERS: &str = "# a comment\n/*\n  Content-Security-Policy: default-src 'self'\n  X-Content-Type-Options: nosniff\n\n/app/*\n  ! Content-Security-Policy\n  Content-Security-Policy: default-src 'self' 'wasm-unsafe-eval'\n\n/app/parts/*\n  Cache-Control: public, max-age=31536000, immutable\n";

#[test]
fn every_rule_that_matches_is_applied_in_order_and_a_header_set_twice_is_joined() {
    let headers = Headers::parse("/*\n  X-A: one\n\n/app/*\n  x-a: two\n  X-B: b\n\n/other/*\n  X-C: c\n").unwrap();
    let resolved = headers.resolve("/app/x");
    assert_eq!(resolved["x-a"], ["one", "two"]);
    assert_eq!(resolved["x-b"], ["b"]);
    assert!(!resolved.contains_key("x-c"));
    assert_eq!(headers.resolve("/other/y")["x-c"], ["c"]);
    assert_eq!(headers.resolve("/elsewhere")["x-a"], ["one"]);
    // The same header twice under one rule is joined too.
    let twice = Headers::parse("/*\n  X-A: one\n  X-A: two\n").unwrap();
    assert_eq!(twice.resolve("/")["x-a"], ["one", "two"]);
}

#[test]
fn a_detach_removes_what_the_rules_before_it_added_and_nothing_after_it() {
    let headers = Headers::parse(SITE_HEADERS).unwrap();
    let app = headers.resolve("/app/index.html");
    assert_eq!(app["content-security-policy"], ["default-src 'self' 'wasm-unsafe-eval'"]);
    assert_eq!(app["x-content-type-options"], ["nosniff"]);
    let page = headers.resolve("/index.html");
    assert_eq!(page["content-security-policy"], ["default-src 'self'"]);
    // A detach with no policy to follow it leaves none; a detach that comes before the rule it would cancel cancels nothing.
    let detached = Headers::parse("/*\n  X-A: one\n\n/app/*\n  ! X-A\n").unwrap();
    assert!(!detached.resolve("/app/x").contains_key("x-a"));
    assert_eq!(detached.resolve("/y")["x-a"], ["one"]);
    let late = Headers::parse("/app/*\n  ! X-A\n\n/*\n  X-A: one\n").unwrap();
    assert_eq!(late.resolve("/app/x")["x-a"], ["one"]);
}

#[test]
fn a_file_pages_would_not_take_is_an_error_with_the_line() {
    let rules = "/a\n  X-A: 1\n".repeat(100);
    Headers::parse(&rules).unwrap();
    let error = Headers::parse(&format!("{rules}/b\n  X-A: 1\n")).unwrap_err();
    assert!(error.contains("100 rules"), "{error}");
    let long = format!("/a\n  X-A: {}\n", "x".repeat(2000));
    assert!(Headers::parse(&long).unwrap_err().contains("2000 characters"));
    assert!(Headers::parse(&format!("/a\n  X-A: {}\n", "x".repeat(1900))).is_ok());
    assert!(Headers::parse("  X-A: 1\n").unwrap_err().contains("before any URL"));
    assert!(Headers::parse("/a\n  no colon here\n").unwrap_err().contains("expected a header"));
    assert!(Headers::parse("/a\n  : value\n").unwrap_err().contains("header name"));
    assert!(Headers::parse("/a\n  Bad Name: 1\n").unwrap_err().contains("header name"));
    assert!(Headers::parse("/a\n  X-A:\n").unwrap_err().contains("no value"));
    assert!(Headers::parse("/a\n  ! X-A: 1\n").unwrap_err().contains("names the header only"));
    assert!(Headers::parse("/a/*\n  X-A: :splat\n").unwrap_err().contains(":splat"));
    assert!(Headers::parse("/:dir\n  X-A: in :dir\n").unwrap_err().contains(":dir"));
    // Text that only looks like a reference is a value.
    assert!(Headers::parse("/a\n  X-A: https://example.invalid:8080/\n").is_ok());
    // A bad pattern says which.
    assert!(Headers::parse("/a/*/b/*\n  X-A: 1\n").unwrap_err().contains("/a/*/b/*"));
}

// ---------------------------------------------------------------------------------------------------------------- Cache-Control, names

#[test]
fn cache_control_is_read_for_what_a_browser_does_with_it() {
    let year = cache::cache_control("public, max-age=31536000, immutable");
    assert_eq!(year, cache::Cache { max_age: Some(YEAR), s_maxage: None, immutable: true, no_store: false, no_cache: false });
    assert!(year.lasts_a_year() && year.long_lived());
    let revalidate = cache::cache_control("public, max-age=0, must-revalidate");
    assert!(!revalidate.lasts_a_year() && !revalidate.long_lived());
    assert!(!cache::cache_control("").long_lived());
    assert!(!cache::cache_control("no-store").long_lived());
    // Any life above 0 keeps a copy; a shared cache's too; immutable alone says the copy does not change.
    assert!(cache::cache_control("max-age=1").long_lived());
    assert!(cache::cache_control("s-maxage=60").long_lived());
    assert!(cache::cache_control("immutable").long_lived());
    // A year without immutable, immutable without a year, and a year that is denied by no-store, are not "for good".
    assert!(!cache::cache_control("max-age=31536000").lasts_a_year());
    assert!(!cache::cache_control("immutable, max-age=31535999").lasts_a_year());
    assert!(!cache::cache_control("immutable, max-age=31536000, no-store").lasts_a_year());
    assert!(cache::cache_control("immutable, max-age=99999999").lasts_a_year());
    // Case, spaces, quotes; a number that is not one is no life.
    let odd = cache::cache_control("  MAX-AGE = \"60\" ,Immutable ,s-maxage=x");
    assert_eq!((odd.max_age, odd.s_maxage, odd.immutable), (Some(60), None, true));
    // A number too large for 64 bits is a very long life, and not none.
    assert_eq!(cache::cache_control("max-age=99999999999999999999999").max_age, Some(u64::MAX));
    assert!(cache::cache_control("no-cache").no_cache);
}

#[test]
fn a_hashed_name_has_sixteen_digits_before_the_extension_or_twelve_between_two_dots() {
    assert_eq!(cache::name_hash("app/parts/diacraft-web-1-886391bcd94eb6f7.wasm"), Some("886391bcd94eb6f7"));
    assert_eq!(cache::name_hash("app/page-288c9feaa511726b.css"), Some("288c9feaa511726b"));
    assert_eq!(cache::name_hash("assets/hashed/cover.51d9795704f7.webp"), Some("51d9795704f7"));
    assert_eq!(cache::name_hash("a-0123456789abcdef.js"), Some("0123456789abcdef"));
    // Not hashes: a short number, upper case, a stem of nothing, a folder that carries the digits, no extension, a part that is not at the end.
    for plain in [
        "app/icon-192.png",
        "app/icon-512.png",
        "app/page-288C9FEAA511726B.css",
        "app/-288c9feaa511726b.css",
        "app/index.html",
        "app/start.js",
        "app/manifest.json",
        "app/diacraft.webmanifest",
        "app/snippets/eframe-d31a8450ea12dfc1/inline0.js",
        "app/page-288c9feaa511726b",
        "app/page-288c9feaa511726b.css.map",
        "app/page-288c9feaa511726.css",
        "app/page-288c9feaa511726bb.css",
        "app/page.288c9feaa511726b.css",
        "assets/hashed/cover.51d9795704f.webp",
        "assets/hashed/cover.51d9795704f7a.webp",
        "assets/hashed/.51d9795704f7.webp",
        "",
    ] {
        assert_eq!(cache::name_hash(plain), None, "{plain}");
    }
}

#[test]
fn a_file_is_served_at_the_addresses_pages_gives_it() {
    assert_eq!(cache::request_paths("index.html"), ["/", "/index.html"]);
    assert_eq!(cache::request_paths("app/index.html"), ["/app/", "/app/index.html"]);
    assert_eq!(cache::request_paths("guides/getting-started.html"), ["/guides/getting-started", "/guides/getting-started.html"]);
    assert_eq!(cache::request_paths("404.html"), ["/404", "/404.html"]);
    assert_eq!(cache::request_paths("app/parts/a.wasm"), ["/app/parts/a.wasm"]);
    assert_eq!(cache::request_paths("styles.css"), ["/styles.css"]);
}

// ------------------------------------------------------------------------------------------------------------------------ the check

/// A folder as the check sees it: names and bytes.
struct Folder(BTreeMap<String, Vec<u8>>, Kind);

impl Folder {
    fn new(kind: Kind) -> Self {
        Folder(BTreeMap::new(), kind)
    }

    fn with(mut self, name: &str, bytes: &[u8]) -> Self {
        self.0.insert(name.to_owned(), bytes.to_vec());
        self
    }

    /// A file named by the hash of its bytes: returns the name it was given.
    fn hashed(&mut self, folder: &str, stem: &str, ext: &str, bytes: &[u8]) -> String {
        let name = format!("{folder}{stem}-{}.{ext}", &sha256(bytes)[..16]);
        self.0.insert(name.clone(), bytes.to_vec());
        name
    }

    fn check(&self, headers: &str) -> Result<Summary, String> {
        let names: Vec<String> = self.0.keys().cloned().collect();
        cache::check(headers, &names, &|name| self.0.get(name).cloned().ok_or_else(|| format!("{name}: not there")), self.1)
    }
}

/// The policy of the pages (`/*`, with the header that follows it), and of the app: the shape of the real `_headers` files, each for a site of its own.
const PAGES_POLICY: &str = "/*\n  Content-Security-Policy: default-src 'self'; script-src 'self'; connect-src 'self' https://api.github.com; img-src 'self'; style-src 'self'; frame-ancestors 'none'\n  X-Content-Type-Options: nosniff\n";
const APP_POLICY: &str = "/*\n  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self'; style-src 'self'; manifest-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'\n  X-Content-Type-Options: nosniff\n";
const KEEP: &str = "  Cache-Control: public, max-age=31536000, immutable\n";

fn headers(policy: &str, rules: &[&str]) -> String {
    let mut text = policy.to_owned();
    for route in rules {
        text.push_str(&format!("\n{route}\n{KEEP}"));
    }
    text
}

/// The pages, like the real folder: the home page, a guide, the 404 page, the style, a picture named by its hash, and `_headers`.
fn pages() -> Folder {
    let mut folder = Folder::new(Kind::Pages)
        .with("index.html", b"home")
        .with("guides/a.html", b"guide")
        .with("404.html", b"missing")
        .with("styles.css", b"body{}")
        .with("_headers", b"not served");
    folder.0.insert("assets/hashed/octopus.".to_owned() + &sha256(b"octopus")[..12] + ".webp", b"octopus".to_vec());
    folder
}

/// The app at the root of its own site: its page, files that are not hashed and files that are.
fn app() -> Folder {
    let mut folder = Folder::new(Kind::App)
        .with("index.html", b"app")
        .with("start.js", b"loader")
        .with("manifest.json", b"{}")
        .with("diacraft.webmanifest", b"{}")
        .with("icon-192.png", b"i192")
        .with("snippets/eframe-d31a8450ea12dfc1/inline0.js", b"export {}")
        .with("_headers", b"not served");
    folder.hashed("parts/", "diacraft-web-1", "wasm", b"module part");
    folder.hashed("assets/", "diacraft-cover", "png", b"cover");
    folder.hashed("", "diacraft-web", "js", b"glue");
    folder.hashed("", "page", "css", b"style");
    folder.hashed("", "diacraft-icon-256", "png", b"icon");
    folder
}

const RULES: [&str; 5] = ["/parts/*", "/assets/*", "/diacraft-web-*.js", "/page-*.css", "/diacraft-icon-256-*.png"];

fn app_rules() -> String {
    headers(APP_POLICY, &RULES)
}

fn pages_rules() -> String {
    headers(PAGES_POLICY, &["/assets/hashed/*"])
}

#[test]
fn a_folder_whose_hashed_files_are_kept_for_good_and_the_rest_asked_about_passes() {
    // Six files named by their hash in the app; the others are its page, its loader, two manifests, an icon and a snippet.
    assert_eq!(app().check(&app_rules()).unwrap(), Summary { kept: 5, revalidated: 6, pages: 1 });
    // The pages: a picture named by its hash; the others are the home page, a guide, the 404 page and the style.
    assert_eq!(pages().check(&pages_rules()).unwrap(), Summary { kept: 1, revalidated: 4, pages: 3 });
}

#[test]
fn a_file_the_headers_are_not_read_for_is_not_counted_and_a_headers_file_alone_has_no_files_to_check() {
    assert_eq!(Folder::new(Kind::Pages).with("_headers", b"x").check(PAGES_POLICY).unwrap(), Summary::default());
}

#[test]
fn a_file_with_no_hash_of_its_own_bytes_gets_no_long_cache() {
    // The rule that reaches too far: `/*` caches the page, the loader and the manifests, and a release would stay in browsers for a year.
    let error = app().check(&headers(APP_POLICY, &["/*", "/parts/*"])).unwrap_err();
    assert!(error.contains("is not named by a hash of its own bytes") || error.contains("2 Cache-Control values"), "{error}");
    for name in ["index.html", "start.js", "manifest.json", "diacraft.webmanifest", "icon-192.png"] {
        let error = app().check(&format!("{}\n/{name}\n{KEEP}", app_rules())).unwrap_err();
        assert!(error.contains(name) && error.contains("is not named by a hash of its own bytes"), "{name}: {error}");
    }
    // Any life above 0, for a browser or a shared cache, is the same fault; so is `immutable` alone.
    for value in ["max-age=1", "public, max-age=3600", "s-maxage=60", "immutable", "public, max-age=31536000, immutable"] {
        let text = format!("{}\n/start.js\n  Cache-Control: {value}\n", app_rules());
        let error = app().check(&text).unwrap_err();
        assert!(error.contains("start.js") && error.contains("is not named by a hash of its own bytes") && error.contains(value), "{value}: {error}");
    }
    // Asked about every time is fine, and so is saying nothing.
    for value in ["max-age=0, must-revalidate", "no-cache", "no-store", "public, max-age=0, must-revalidate"] {
        app().check(&format!("{}\n/start.js\n  Cache-Control: {value}\n", app_rules())).unwrap();
    }
    // The pages are asked about too: the home page kept for a year would hide the next release.
    let error = pages().check(&format!("{}\n/\n{KEEP}", pages_rules())).unwrap_err();
    assert!(error.contains("index.html") && error.contains("is not named by a hash of its own bytes"), "{error}");
}

#[test]
fn the_snippets_are_asked_about_because_their_folder_is_a_hash_of_a_crate_and_not_of_the_file() {
    let error = app().check(&format!("{}\n/snippets/*\n{KEEP}", app_rules())).unwrap_err();
    assert!(error.contains("snippets/eframe-d31a8450ea12dfc1/inline0.js") && error.contains("is not named by a hash"), "{error}");
}

#[test]
fn a_name_that_carries_a_hash_that_is_not_its_own_bytes_is_not_kept_for_good() {
    let mut folder = app();
    // Trunk's old name, or a copy of a file after its content changed: the digits are there, and they are another file's.
    folder.0.insert("page-0123456789abcdef.css".into(), b"other style".to_vec());
    folder.0.remove(&format!("page-{}.css", &sha256(b"style")[..16]));
    let error = folder.check(&app_rules()).unwrap_err();
    assert!(
        error.contains("page-0123456789abcdef.css") && error.contains("0123456789abcdef") && error.contains(&sha256(b"other style")[..16]),
        "{error}"
    );
    // The same name with no long cache is only a name.
    folder.check(&headers(APP_POLICY, &["/parts/*", "/assets/*", "/diacraft-web-*.js", "/diacraft-icon-256-*.png"])).unwrap();
}

#[test]
fn a_file_named_by_its_own_hash_must_be_kept_for_a_year_and_marked_immutable() {
    // No rule for the three files at the app's root: each is asked about on every visit.
    let error = app().check(&headers(APP_POLICY, &["/parts/*", "/assets/*"])).unwrap_err();
    assert!(error.contains("is named by the hash of its own bytes") && error.contains("diacraft-icon-256-"), "{error}");
    for (missing, file) in [
        ("/parts/*", "parts/diacraft-web-1-"),
        ("/assets/*", "assets/diacraft-cover-"),
        ("/diacraft-web-*.js", "diacraft-web-"),
        ("/page-*.css", "page-"),
        ("/diacraft-icon-256-*.png", "diacraft-icon-256-"),
    ] {
        let rules: Vec<&str> = RULES.iter().copied().filter(|r| *r != missing).collect();
        let error = app().check(&headers(APP_POLICY, &rules)).unwrap_err();
        assert!(error.contains(file) && error.contains("is named by the hash of its own bytes"), "{missing}: {error}");
    }
    // The pages' pictures too.
    let error = pages().check(PAGES_POLICY).unwrap_err();
    assert!(error.contains("assets/hashed/octopus.") && error.contains("is named by the hash of its own bytes"), "{error}");
    // A life short of a year, or no `immutable`, is not for good.
    for weak in ["max-age=31536000", "immutable", "public, max-age=86400, immutable", "public, max-age=31535999, immutable", "no-store"] {
        let text =
            app_rules().replace("/parts/*\n  Cache-Control: public, max-age=31536000, immutable", &format!("/parts/*\n  Cache-Control: {weak}"));
        let error = app().check(&text).unwrap_err();
        assert!(error.contains("parts/diacraft-web-1-") && error.contains("is named by the hash of its own bytes"), "{weak}: {error}");
    }
}

#[test]
fn two_cache_headers_for_one_address_are_refused_because_pages_joins_them() {
    // The app's parts are matched by `/parts/*` and by a second rule that sets a cache header there: it would be sent as `a, b`.
    let text = format!("{}\n/parts/diacraft-web-*\n  Cache-Control: public, max-age=0, must-revalidate\n", app_rules());
    let error = app().check(&text).unwrap_err();
    assert!(
        error.contains("2 Cache-Control values") && error.contains("public, max-age=31536000, immutable, public, max-age=0, must-revalidate"),
        "{error}"
    );
    // A rule for a file the other rules do not reach makes one.
    let text = format!("{}\n/start.js\n  Cache-Control: no-cache\n", app_rules());
    app().check(&text).unwrap();
    // A detach first takes the year away from the file the rule before gave it.
    let text = format!("{}\n/parts/*\n  ! Cache-Control\n  Cache-Control: public, max-age=0, must-revalidate\n", app_rules());
    let error = app().check(&text).unwrap_err();
    assert!(error.contains("is named by the hash of its own bytes"), "the detach took the year away: {error}");
}

#[test]
fn every_page_has_one_policy_and_it_is_the_sites_own() {
    // Lost: no rule gives the pages one.
    let none = pages_rules().replace("/*\n  Content-Security-Policy", "/nothing\n  X-Other");
    let error = pages().check(&none).unwrap_err();
    assert!(error.contains("no content security policy"), "{error}");
    let none = app_rules().replace("/*\n  Content-Security-Policy", "/nothing\n  X-Other");
    let error = app().check(&none).unwrap_err();
    assert!(error.contains("index.html") && error.contains("no content security policy"), "{error}");
    // Two policies joined: a second rule that sets one.
    let both = format!("{}\n/index.html\n  Content-Security-Policy: default-src 'self'\n", app_rules());
    let error = app().check(&both).unwrap_err();
    assert!(error.contains("index.html") && error.contains("2 content security policies"), "{error}");
    // The app's page under the pages' policy would not run; the pages under the app's policy would be looser than they are held to. The kind of the folder says which is held.
    let error = app().check(&headers(PAGES_POLICY, &RULES)).unwrap_err();
    assert!(error.contains("index.html") && error.contains("script-src"), "{error}");
    let error = pages().check(&headers(APP_POLICY, &["/assets/hashed/*"])).unwrap_err();
    assert!(error.contains("html") && error.contains("script-src"), "{error}");
    let loose = pages_rules().replace(
        "script-src 'self'; connect-src 'self' https://api.github.com",
        "script-src 'self' 'unsafe-inline'; connect-src 'self' https://api.github.com",
    );
    let error = pages().check(&loose).unwrap_err();
    assert!(error.contains("script-src"), "{error}");
}

#[test]
fn a_page_is_checked_at_each_address_it_is_served_at() {
    // A rule for the address with no `.html` and a rule for the folder: each page is reached by both forms.
    let text = format!("{}\n/guides/a\n  ! Content-Security-Policy\n", pages_rules());
    let error = pages().check(&text).unwrap_err();
    assert!(error.contains("guides/a.html") && error.contains("/guides/a") && error.contains("no content security policy"), "{error}");
    let text = format!("{}\n/guides/a.html\n  ! Content-Security-Policy\n", pages_rules());
    assert!(pages().check(&text).unwrap_err().contains("/guides/a.html"));
    // The app's page is served at `/` and at `/index.html`, and both are checked.
    let text = format!("{}\n/index.html\n  ! Content-Security-Policy\n", app_rules());
    assert!(app().check(&text).unwrap_err().contains("/index.html"));
}

#[test]
fn a_headers_file_that_cannot_be_read_is_an_error_naming_the_line() {
    let error = app().check("/a/*/b/*\n  X-A: 1\n").unwrap_err();
    assert!(error.contains("one splat"), "{error}");
}

#[test]
fn a_file_that_cannot_be_read_and_too_many_files_are_errors() {
    let names = vec!["a-0123456789abcdef.js".to_owned()];
    let error = cache::check(&app_rules(), &names, &|name| Err(format!("{name}: gone")), Kind::App).unwrap_err();
    assert!(error.contains("gone"), "{error}");
    let many: Vec<String> = (0..4097).map(|n| format!("f{n}.txt")).collect();
    let error = cache::check(&app_rules(), &many, &|_| Ok(Vec::new()), Kind::App).unwrap_err();
    assert!(error.contains("4096 files"), "{error}");
}
