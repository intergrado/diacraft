//! Tests of the web app's part of the site checks: the rewrite of Trunk's page, the reading of a WebAssembly module's memory, the `_headers` block
//! of the app, the copy into the output folders (the module cut into parts, the data beside them, the manifest and the loader: `parts_tests.rs` tests those on their own) and the
//! check of what was copied.

use super::app::{self, Web};
use super::parts;
use super::{Mode, check, headers, parse};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A page as Trunk writes it, with its inline start script (the file names are made up).
pub(super) const TRUNK_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>DiaCraft</title>
  <link rel="icon" href="./diacraft-icon-256-00112233445566aa.png" integrity="sha384-x"/>
  <link rel="stylesheet" href="./page-00112233445566bb.css" integrity="sha384-y"/>
  <link rel="manifest" href="diacraft.webmanifest" />

<script type="module">
import init, * as bindings from './diacraft-web-00112233445566cc.js';

const wasm = await init({ module_or_path: './diacraft-web-00112233445566dd_bg.wasm' });

window.wasmBindings = bindings;

</script>
<link rel="modulepreload" href="./diacraft-web-00112233445566cc.js" crossorigin="anonymous" integrity="sha384-z"><link rel="preload" href="./diacraft-web-00112233445566dd_bg.wasm" crossorigin="anonymous" integrity="sha384-w" as="fetch" type="application/wasm"></head>
<body>
  <canvas id="diacraft_canvas"></canvas>
  <div id="diacraft_loading">Loading DiaCraft&hellip;</div>
</body>
</html>
"#;

/// The names Trunk gives the files at the app's root in [`TRUNK_PAGE`]: a hash of 16 digits (its own), which `assemble` replaces by the SHA-256 of the file.
pub(super) const TRUNK_ICON: &str = "diacraft-icon-256-00112233445566aa.png";
pub(super) const TRUNK_CSS: &str = "page-00112233445566bb.css";
pub(super) const TRUNK_GLUE: &str = "diacraft-web-00112233445566cc.js";
pub(super) const TRUNK_WASM: &str = "diacraft-web-00112233445566dd_bg.wasm";

/// The content of the glue Trunk made, in the tests.
pub(super) const GLUE: &str =
    "import * as inline from './snippets/eframe-1/inline0.js';\nexport default async function init(o) { return fetch(o.module_or_path); }\n";

/// The pages' `_headers` (`site/_headers`), in short: the strict policy and the three headers under `/*`, and nothing of the app's.
pub(super) const PAGES_HEADERS: &str = "/*\n  Content-Security-Policy: default-src 'self'; script-src 'self'; connect-src https://api.github.com; img-src 'self'; style-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'\n  X-Content-Type-Options: nosniff\n  Referrer-Policy: no-referrer\n  Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()\n";

/// The app's `_headers` (`apps/diacraft-web/_headers`), in short: its own policy and the three headers under `/*`, and the rules that cache the app's hashed files for good.
pub(super) const HEADERS: &str = "/*\n  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self'; style-src 'self'; manifest-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'\n  X-Content-Type-Options: nosniff\n  Referrer-Policy: no-referrer\n  Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()\n\n/parts/*\n  Cache-Control: public, max-age=31536000, immutable\n\n/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n\n/diacraft-web-*.js\n  Cache-Control: public, max-age=31536000, immutable\n\n/page-*.css\n  Cache-Control: public, max-age=31536000, immutable\n\n/diacraft-icon-256-*.png\n  Cache-Control: public, max-age=31536000, immutable\n";

/// A WebAssembly module with only a header: valid, and no memory at all.
pub(super) const EMPTY_WASM: &[u8] = b"\0asm\x01\0\0\0";

pub(super) struct Dir(PathBuf);

impl Dir {
    pub(super) fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("diacraft-site-app-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        Dir(path)
    }

    pub(super) fn write(&self, name: &str, content: impl AsRef<[u8]>) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// ------------------------------------------------------------------------------------------------------------------------- the page rewrite

#[test]
fn an_inline_module_script_becomes_a_file_with_the_same_text_and_the_page_loads_it_by_src() {
    let (page, files) = app::externalise(TRUNK_PAGE).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, "start.js");
    assert!(files[0].1.starts_with(&format!("import init, * as bindings from './{TRUNK_GLUE}';")), "{}", files[0].1);
    assert!(files[0].1.contains(&format!("await init({{ module_or_path: './{TRUNK_WASM}' }})")), "the text is Trunk's, unchanged");
    assert!(files[0].1.ends_with("window.wasmBindings = bindings;\n"), "{:?}", files[0].1);
    assert!(page.contains("<script type=\"module\" src=\"./start.js\"></script>"), "{page}");
    assert!(!page.contains("import init"), "nothing of the script is left in the page");
    // The rest of the page is as it was.
    assert!(page.contains(&format!("<link rel=\"modulepreload\" href=\"./{TRUNK_GLUE}\"")) && page.contains("<canvas id=\"diacraft_canvas\">"));
    assert!(page.starts_with("<!DOCTYPE html>") && page.ends_with("</html>\n"));
}

#[test]
fn a_page_with_no_inline_script_comes_back_as_it_was_and_a_script_with_a_src_is_kept() {
    let plain = "<html><head><title>x</title></head><body><p>no script, and a <scripted> word</p></body></html>";
    assert_eq!(app::externalise(plain).unwrap(), (plain.to_owned(), Vec::new()));
    let linked = "<head><script type=\"module\" src=\"./a.js\"></script></head>";
    let (page, files) = app::externalise(linked).unwrap();
    assert_eq!(page, linked);
    assert!(files.is_empty());
}

#[test]
fn several_inline_scripts_get_a_file_each_in_order_and_too_many_or_a_broken_tag_is_an_error() {
    let (page, files) = app::externalise("<SCRIPT>one()</SCRIPT><p></p><script type='module'>two()</script>").unwrap();
    assert_eq!(files.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect::<Vec<_>>(), [("start.js", "one()\n"), ("start-2.js", "two()\n")]);
    assert_eq!(page, "<script src=\"./start.js\"></script><p></p><script type=\"module\" src=\"./start-2.js\"></script>");
    let many = "<script>x()</script>".repeat(9);
    assert!(app::externalise(&many).unwrap_err().contains("more than 8 inline scripts"));
    assert!(app::externalise("<script>never ends").unwrap_err().contains("no end tag"));
    assert!(app::externalise("<script src=\"a.js\">body()</script>").unwrap_err().contains("both a src and a body"));
    assert!(app::externalise("<script type=\"module\"").unwrap_err().contains("unterminated"));
}

// --------------------------------------------------------------------------------------------------------------------------- WebAssembly memory

/// A custom section (id 0) holding `text`: what a module's data is, to the check.
fn custom(text: &[u8]) -> Vec<u8> {
    let mut payload = vec![1, b'x'];
    payload.extend(text);
    // The size as two bytes of LEB128: the payloads here are under 16,384 bytes.
    assert!(payload.len() < 16_384);
    let mut section = vec![0u8, 0x80 | (payload.len() % 128) as u8, (payload.len() / 128) as u8];
    section.extend(payload);
    section
}

/// A module with the given sections, as (id, payload).
pub(super) fn module(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = EMPTY_WASM.to_vec();
    for (id, payload) in sections {
        bytes.push(*id);
        // The payloads of these tests are short: one byte of size.
        assert!(payload.len() < 128);
        bytes.push(u8::try_from(payload.len()).unwrap());
        bytes.extend(payload);
    }
    bytes
}

#[test]
fn leb128_reads_numbers_and_refuses_what_is_cut_off_or_too_long() {
    let read = |bytes: &[u8]| {
        let mut at = 0;
        let value = app::leb128(bytes, &mut at);
        (value, at)
    };
    assert_eq!(read(&[0x00]), (Some(0), 1));
    assert_eq!(read(&[0x7f]), (Some(127), 1));
    assert_eq!(read(&[0x80, 0x01]), (Some(128), 2));
    assert_eq!(read(&[0xe5, 0x8e, 0x26]), (Some(624_485), 3));
    assert_eq!(read(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]), (Some(u64::MAX), 10));
    // Cut off, ten bytes with the continuation bit still set, and a tenth byte that does not fit 64 bits.
    assert_eq!(read(&[0x80]).0, None);
    assert_eq!(read(&[]).0, None);
    assert_eq!(read(&[0x80; 11]).0, None);
    assert_eq!(read(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02]).0, None);
    // A start past the end is no number and does not move.
    let mut at = 5;
    assert_eq!(app::leb128(&[0x01], &mut at), None);
    assert_eq!(at, 5);
}

#[test]
fn a_shared_memory_is_found_in_the_memory_section_and_in_an_import() {
    // Memory section: one memory. Flags 0x03 is shared with a maximum, 0x01 a maximum only, 0x00 neither.
    assert!(app::wasm_shared_memory(&module(&[(5, vec![1, 0x03, 17, 65])])).unwrap());
    assert!(!app::wasm_shared_memory(&module(&[(5, vec![1, 0x01, 17, 65])])).unwrap());
    assert!(!app::wasm_shared_memory(&module(&[(5, vec![1, 0x00, 17])])).unwrap());
    // Import section: a function (`a.f`, type 0), a global, then a memory `env.memory`.
    let mut imports = vec![3u8];
    imports.extend([1, b'a', 1, b'f', 0x00, 0]);
    imports.extend([1, b'a', 1, b'g', 0x03, 0x7f, 0x01]);
    let shared = [imports.clone(), vec![3, b'e', b'n', b'v', 6, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x03, 1, 2]].concat();
    let plain = [imports, vec![3, b'e', b'n', b'v', 6, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x00, 1]].concat();
    assert!(app::wasm_shared_memory(&module(&[(2, shared)])).unwrap());
    assert!(!app::wasm_shared_memory(&module(&[(2, plain)])).unwrap());
    // A module with other sections only, and one with none.
    assert!(!app::wasm_shared_memory(&module(&[(1, vec![0]), (10, vec![0])])).unwrap());
    assert!(!app::wasm_shared_memory(EMPTY_WASM).unwrap());
}

#[test]
fn a_cut_off_or_hostile_module_is_an_error_and_not_a_panic() {
    // A section that claims more bytes than the module has.
    let mut bytes = EMPTY_WASM.to_vec();
    bytes.extend([5, 0x7f, 1, 0x03]);
    assert!(app::wasm_shared_memory(&bytes).unwrap_err().contains("cut off"));
    // A size that is a number too long to be one, and a count of imports with none after it.
    let mut bytes = EMPTY_WASM.to_vec();
    bytes.extend([2, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80]);
    assert!(app::wasm_shared_memory(&bytes).is_err());
    assert!(app::wasm_shared_memory(&module(&[(2, vec![5])])).is_err());
    // An import of a kind there is none of.
    assert!(app::wasm_shared_memory(&module(&[(2, vec![1, 1, b'a', 1, b'f', 0x09, 0])])).unwrap_err().contains("kind 9"));
    // Every prefix of a real shape of module is read to an answer or an error.
    let whole = module(&[(2, vec![1, 1, b'a', 1, b'f', 0x00, 0]), (5, vec![1, 0x03, 1, 2])]);
    for cut in 0..whole.len() {
        let _ = app::wasm_shared_memory(whole.get(..cut).unwrap());
    }
    assert!(app::wasm_shared_memory(&whole).unwrap());
}

// ----------------------------------------------------------------------------------------------------------------------------- the headers

#[test]
fn the_apps_file_is_checked_as_its_own_and_the_pages_policy_stays_strict() {
    headers::check_app(HEADERS).unwrap();
    // The pages' file is the pages': the app's policy is not theirs, and theirs is not the app's.
    assert!(headers::check_pages(PAGES_HEADERS, false).is_ok());
    assert!(headers::check_app(PAGES_HEADERS).unwrap_err().contains("script-src"));
    assert!(headers::check_pages(HEADERS, false).unwrap_err().contains("script-src"));
    // The hashed files of the app must be cached for good, or a visitor asks about 17 MB on every visit.
    for route in super::headers::APP_CACHE_ROUTES {
        let without = HEADERS.replace(&format!("\n\n{route}\n  Cache-Control: public, max-age=31536000, immutable"), "");
        assert_ne!(without, HEADERS, "{route}");
        let error = headers::check_app(&without).unwrap_err();
        assert!(error.contains(route) && error.contains("must be cached for good"), "{route}: {error}");
    }
    // The pages' file has nothing of the app's: a rule for a path of the app is refused, and so is a policy under any route but `/*`.
    for app_rule in ["/app/*\n  Cache-Control: public, max-age=31536000, immutable\n", "/app\n  X-Other: 1\n"] {
        let error = headers::check_pages(&format!("{PAGES_HEADERS}\n{app_rule}"), false).unwrap_err();
        assert!(error.contains("a site of its own"), "{app_rule}: {error}");
    }
    let error = headers::check_pages(&format!("{PAGES_HEADERS}\n/guides/*\n  Content-Security-Policy: default-src 'self'\n"), false).unwrap_err();
    assert!(error.contains("only /* sets one"), "{error}");
}

#[test]
fn the_apps_policy_is_exactly_the_one_it_needs_and_nothing_looser() {
    let app_csp = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self'; style-src 'self'; manifest-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
    for (from, to, diagnostic) in [
        ("'self' 'wasm-unsafe-eval'", "'self' 'wasm-unsafe-eval' 'unsafe-inline'", "script-src"),
        ("'self' 'wasm-unsafe-eval'", "'self' 'unsafe-eval'", "script-src"),
        ("'self' 'wasm-unsafe-eval'", "'self'", "script-src"),
        ("connect-src 'self'", "connect-src 'self' https://api.github.com", "connect-src"),
        ("style-src 'self'", "style-src 'self' 'unsafe-inline'", "style-src"),
        ("img-src 'self'", "img-src 'self' data:", "img-src"),
        ("; form-action 'none'", "", "missing form-action"),
        ("; manifest-src 'self'", "", "missing manifest-src"),
        ("manifest-src 'self'", "manifest-src 'self' https://cdn.invalid", "manifest-src"),
        ("; form-action 'none'", "; form-action 'none'; worker-src blob:", "worker-src"),
        ("; base-uri 'none'", "; base-uri 'none'; base-uri 'none'", "duplicate"),
    ] {
        let headers = HEADERS.replace(app_csp, &app_csp.replace(from, to));
        let error = headers::check_app(&headers).unwrap_err();
        assert!(error.contains(diagnostic), "{from} -> {to}: {error}");
    }
}

#[test]
fn the_apps_file_sets_its_policy_and_three_headers_for_every_path_and_a_cache_header_for_its_hashed_files_and_nothing_else() {
    // Under `/*`: the policy and the three headers, each once. Another header would be cross-origin isolation, which the app does not use, or a looser one of the pages'.
    for extra in [
        "Cross-Origin-Embedder-Policy: require-corp",
        "Cross-Origin-Opener-Policy: same-origin",
        "X-Frame-Options: DENY",
        "Cache-Control: public, max-age=60",
    ] {
        let error = headers::check_app(&HEADERS.replacen("/*\n", &format!("/*\n  {extra}\n"), 1)).unwrap_err();
        assert!(error.contains("nothing else"), "{extra}: {error}");
    }
    for (header, missing) in [
        ("  Content-Security-Policy", "content-security-policy"),
        ("  X-Content-Type-Options", "x-content-type-options"),
        ("  Referrer-Policy", "referrer-policy"),
        ("  Permissions-Policy", "permissions-policy"),
    ] {
        let without: String = HEADERS.lines().filter(|l| !l.starts_with(header)).map(|l| format!("{l}\n")).collect();
        let error = headers::check_app(&without).unwrap_err();
        assert!(error.contains(&format!("missing {missing}")), "{header}: {error}");
        let twice = HEADERS.replacen(header, &format!("{header}: x\n{header}"), 1);
        assert!(headers::check_app(&twice).is_err(), "{header}");
    }
    assert!(headers::check_app(&HEADERS.replace("no-referrer", "unsafe-url")).unwrap_err().contains("invalid referrer-policy"));
    // The rules for the hashed files: a cache header and nothing else, and for good.
    for extra in ["Content-Security-Policy: default-src 'self'", "X-Content-Type-Options: nosniff", "Content-Type: text/plain"] {
        let headers = HEADERS.replace("/assets/*\n", &format!("/assets/*\n  {extra}\n"));
        assert!(headers::check_app(&headers).unwrap_err().contains("sets a cache header and nothing else"), "{extra}");
    }
    for weak in ["public, max-age=60", "public, max-age=31536000", "public, immutable", "no-store"] {
        let headers =
            HEADERS.replace("/parts/*\n  Cache-Control: public, max-age=31536000, immutable", &format!("/parts/*\n  Cache-Control: {weak}"));
        let error = headers::check_app(&headers).unwrap_err();
        assert!(error.contains("/parts/*") && error.contains("immutable caching for at least 30 days"), "{weak}: {error}");
    }
    // No other route, and no detach (the app is no block of the pages' file any more: there is no policy of the pages to take away).
    let error = headers::check_app(&format!("{HEADERS}\n/start.js\n  Cache-Control: public, max-age=31536000, immutable\n")).unwrap_err();
    assert!(error.contains("a rule for /start.js") && error.contains("and no other"), "{error}");
    for bad in ["/*\n  ! Content-Security-Policy\n", "/parts/*\n  ! Cache-Control\n"] {
        let error = headers::check_app(&format!("{HEADERS}\n{bad}")).unwrap_err();
        assert!(error.contains("no header is detached"), "{bad}: {error}");
        assert!(headers::check_pages(&format!("{PAGES_HEADERS}\n{bad}"), false).unwrap_err().contains("no header is detached"), "{bad}");
    }
    let elsewhere = HEADERS.replace("/parts/*", "/other/*");
    assert!(headers::check_app(&elsewhere).unwrap_err().contains("a rule for /other/*"));
    assert!(headers::check_app("/*\n  not a header\n").is_err());
}

// -------------------------------------------------------------------------------------------------------------------------------- arguments

#[test]
fn the_web_flags_are_read_and_a_bad_line_gets_the_usage() {
    assert_eq!(parse(&[]).unwrap(), Mode::Check);
    assert_eq!(parse(&["--write"]).unwrap(), Mode::Write);
    assert_eq!(parse(&["--release"]).unwrap(), Mode::Release);
    assert_eq!(parse(&["--web"]).unwrap(), Mode::Web(Web::default()));
    assert_eq!(
        parse(&["--web", "--dist", "build", "--out", "public", "--app-out", "app-public", "--app-url", "http://127.0.0.1:8100/"]).unwrap(),
        Mode::Web(Web {
            dist: Some("build".into()),
            out: Some("public".into()),
            app_out: Some("app-public".into()),
            app_url: Some("http://127.0.0.1:8100/".into())
        })
    );
    for bad in [
        &["--dist", "x"][..],
        &["--web", "--dist"],
        &["--web", "--dist", "--out"],
        &["--web", "--dist", "a", "--dist", "b"],
        &["--web", "--app-out", "a", "--app-out", "b"],
        &["--web", "--app-url"],
        &["--web", "--app-url", "a", "--app-url", "b"],
        &["--web", "--other"],
        &["--web", "--out", ""],
        &["--write", "--web"],
        &["--web", "--release"],
        &["--app-url", "https://x.example"],
    ] {
        let error = parse(bad).unwrap_err();
        assert!(error.starts_with("usage: cargo xtask site"), "{bad:?}: {error}");
    }
}

// --------------------------------------------------------------------------------------------------------- the pages' links to the app

pub(super) const PAGE: &str = r##"<!DOCTYPE html><html lang="en"><head><title>Drawing</title><meta name="description" content="A drawing guide." />
<link rel="canonical" href="https://example.invalid/" /><meta property="og:url" content="https://example.invalid/" />
<meta property="og:image" content="https://example.invalid/assets/social.jpg" /></head><body><main><a data-app="" href="https://app.example.invalid/">Open the app</a></main></body></html>"##;

/// A second page of the pages, at `/guides/one`.
pub(super) const GUIDE: &str = r##"<!DOCTYPE html><html lang="en"><head><title>One</title><meta name="description" content="A guide." />
<link rel="canonical" href="https://example.invalid/guides/one" /><meta property="og:url" content="https://example.invalid/guides/one" />
<meta property="og:image" content="https://example.invalid/assets/social.jpg" /></head><body><main><p>One.</p></main></body></html>"##;

fn site(headers: &str) -> Dir {
    let dir = Dir::new("pages");
    dir.write("index.html", PAGE);
    dir.write("_headers", headers);
    dir.write("assets/social.jpg", "jpg");
    dir
}

#[test]
fn a_page_may_link_the_app_at_its_own_address_and_a_path_of_the_pages_that_is_no_file_is_a_missing_link() {
    let dir = site(PAGES_HEADERS);
    check(dir.path(), "https://example.invalid").unwrap();
    // The app is a site of its own: its address is another site's, and what a page holds of it is a link, which is checked by `--web` and not here.
    dir.write("index.html", PAGE.replace("https://app.example.invalid/", "https://app.example.invalid/index.html"));
    check(dir.path(), "https://example.invalid").unwrap();
    // The old address, a folder of the pages, is no file there: a page that still has it has it wrong.
    for link in ["/app/", "/app/index.html", "/app/start.js"] {
        dir.write("index.html", PAGE.replace("https://app.example.invalid/", link));
        let error = check(dir.path(), "https://example.invalid").unwrap_err();
        assert!(error.contains("missing") && error.contains("app/"), "{link}: {error}");
    }
    // Another site is refused as before, and the app's host is not an image the page loads from there.
    dir.write("index.html", PAGE.replace("app.example.invalid", "other.example.invalid"));
    assert!(check(dir.path(), "https://example.invalid").unwrap_err().contains("unapproved"));
    dir.write(
        "index.html",
        PAGE.replace(
            "<a data-app=\"\" href=\"https://app.example.invalid/\">Open the app</a>",
            "<img src=\"https://app.example.invalid/a.png\" alt=\"x\" width=\"1\" height=\"1\" />",
        ),
    );
    assert!(check(dir.path(), "https://example.invalid").unwrap_err().contains("external asset"));
}

#[test]
fn the_pages_headers_have_no_rule_for_the_app() {
    // The block the app had in the pages' file (a detach of the pages' policy, then its own) is refused as a rule for the app, and as a detach.
    let dir = site(&format!("{PAGES_HEADERS}\n/app/*\n  Cache-Control: public, max-age=31536000, immutable\n"));
    let error = check(dir.path(), "https://example.invalid").unwrap_err();
    assert!(error.contains("a site of its own"), "{error}");
    let dir =
        site(&format!("{PAGES_HEADERS}\n/app/*\n  ! Content-Security-Policy\n  Content-Security-Policy: default-src 'self' 'wasm-unsafe-eval'\n"));
    let error = check(dir.path(), "https://example.invalid").unwrap_err();
    assert!(error.contains("no header is detached"), "{error}");
}

// -------------------------------------------------------------------------------------------------------------------------- the check

/// A finished app folder, the root of the app's own site: what `assemble` makes of a Trunk build and the repository's own loader and data: the page, the module in parts, the data beside them,
/// the manifest, the loader, and the headers.
fn out_dir() -> Dir {
    out_dir_with(EMPTY_WASM)
}

/// [`out_dir`] with `module` as the module Trunk made.
pub(super) fn out_dir_with(module: &[u8]) -> Dir {
    let root = repo();
    root.write(&format!("dist/web/{TRUNK_WASM}"), module);
    let out = Dir::new("out");
    app::assemble(root.path(), &root.path().join("dist/web"), out.path()).unwrap();
    out
}

#[test]
fn a_finished_app_folder_passes_and_reports_each_file_with_its_size() {
    let out = out_dir();
    let report = app::check(out.path()).unwrap();
    let names: Vec<&str> = report.files.iter().map(|(n, _)| n.as_str()).collect();
    // The names with a hash in them are told by their shape: the module is one part (it is a header), and the data is three files.
    assert_eq!(names.len(), 15, "{names:?}");
    for plain in [
        "_headers",
        "diacraft.webmanifest",
        "icon-192.png",
        "icon-512.png",
        "index.html",
        "manifest.json",
        "snippets/eframe-1/inline0.js",
        "start.js",
    ] {
        assert!(names.contains(&plain), "{plain} in {names:?}");
    }
    // Every other name is a hash of the file's own bytes: the parts and the data, and the three files Trunk made, which `assemble` named again.
    for hashed in [
        "assets/diacraft-cover-",
        "assets/diacraft-splash-",
        "assets/third-party-licenses-web-",
        "parts/diacraft-web-1-",
        "diacraft-web-",
        "page-",
        "diacraft-icon-256-",
    ] {
        assert!(names.iter().any(|n| n.starts_with(hashed)), "{hashed} in {names:?}");
    }
    for name in names
        .iter()
        .filter(|n| !["_headers", "diacraft.webmanifest", "icon-192.png", "icon-512.png", "index.html", "manifest.json", "start.js"].contains(n))
    {
        if name.starts_with("snippets/") {
            continue;
        }
        let digits = super::cache::name_hash(name).unwrap_or_else(|| panic!("{name} has no hash in its name"));
        assert!(parts::sha256(&fs::read(out.path().join(name)).unwrap()).starts_with(digits), "{name}: the hash is not its own");
    }
    assert!(!names.iter().any(|n| n.ends_with("_bg.wasm")), "the module whole is not in the app: {names:?}");
    assert_eq!(report.total, report.files.iter().map(|(_, b)| *b).sum::<u64>());
    assert_eq!(report.module, super::parts::Module { bytes: 8, parts: vec![8] });
    let lines = report.lines();
    assert!(lines.iter().any(|l| l.starts_with("site: app/parts/diacraft-web-1-") && l.ends_with("0.00 MiB (8 bytes)")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("the module is 0.00 MiB (8 bytes) in 1 parts, none over 8.00 MiB")), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("site: app: 15 files")), "{lines:?}");
    assert!(lines.last().unwrap().starts_with("site: caching: "), "{lines:?}");
}

/// What `assemble` names the files of [`repo`] that Trunk hashed: `<stem>-<the first 16 digits of the SHA-256 of the file>.<ext>`.
fn icon_out() -> String {
    format!("diacraft-icon-256-{}.png", &parts::sha256(b"png")[..16])
}

pub(super) fn glue_out() -> String {
    format!("diacraft-web-{}.js", &parts::sha256(GLUE.as_bytes())[..16])
}

/// The name in the app's folder that begins with `prefix` and ends with `ext`: there is one.
fn app_file(out: &Dir, prefix: &str, ext: &str) -> String {
    let names: Vec<String> = fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with(prefix) && n.ends_with(ext))
        .collect();
    assert_eq!(names.len(), 1, "{prefix}*{ext}: {names:?}");
    names[0].clone()
}

/// The path from `out` of the glue, the style and the icon of the finished folder.
fn glue(out: &Dir) -> String {
    app_file(out, "diacraft-web-", ".js")
}

fn css(out: &Dir) -> String {
    app_file(out, "page-", ".css")
}

/// The style as the page names it.
fn css_ref(out: &Dir) -> String {
    format!("./{}", app_file(out, "page-", ".css"))
}

/// A change to the finished folder, and the words the check fails with.
fn refuses(change: impl Fn(&Dir), diagnostic: &str) {
    let out = out_dir();
    change(&out);
    let error = app::check(out.path()).unwrap_err();
    assert!(error.contains(diagnostic), "expected {diagnostic:?} in {error:?}");
}

#[test]
fn the_check_fails_on_a_page_the_policy_would_break_or_that_loads_what_is_not_there() {
    refuses(|o| o.write("index.html", TRUNK_PAGE), "inline <script>");
    // The page loads the loader by `start.js`: a page that is already the loader's is as the check wants it, and one that goes without its preloads is refused (the loader's fetch of
    // each part would come twice).
    refuses(
        |o| {
            let page = fs::read_to_string(o.path().join("index.html")).unwrap();
            let cut = page.find("<link rel=\"preload\"").unwrap();
            o.write("index.html", page.get(..cut).unwrap().to_owned() + "</head>\n<body><canvas id=\"diacraft_canvas\"></canvas></body></html>");
        },
        "the page preloads",
    );
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("<canvas", "<style>x{}</style><canvas")),
        "<style",
    );
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("<canvas ", "<canvas style=\"x\" ")),
        " style=",
    );
    refuses(|o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace(&css_ref(o), "./gone.css")), "gone.css");
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace(&css_ref(o), "https://cdn.invalid/x.css")),
        "not a file of the app",
    );
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace(&css_ref(o), "../page.css")),
        "not a file of the app",
    );
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("<canvas id=\"diacraft_canvas\">", "<canvas>")),
        "diacraft_canvas",
    );
    refuses(
        |o| {
            o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("<link rel=\"stylesheet\"", "<link rel=\"other\""))
        },
        "stylesheet",
    );
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("src=\"./start.js\"", "src=\"./other.js\"")),
        "other.js",
    );
    refuses(|o| fs::remove_file(o.path().join("index.html")).unwrap(), "no index.html");
    // A script with a src first and an inline one after it: every tag is looked at, not the first alone.
    refuses(
        |o| {
            let page = fs::read_to_string(o.path().join("index.html")).unwrap();
            o.write("index.html", page.replace("</body>", "<script>run()</script></body>"));
        },
        "inline <script>",
    );
}

/// The web app manifest: the page links it, it is the app's own and asks for nothing beside what makes the app installable, and its icons are the sizes they say.
#[test]
fn the_check_holds_the_web_app_manifest_and_its_icons() {
    // The page must link it, and the file must be there.
    refuses(
        |o| o.write("index.html", fs::read_to_string(o.path().join("index.html")).unwrap().replace("rel=\"manifest\"", "rel=\"other\"")),
        "does not link its manifest",
    );
    refuses(
        |o| {
            o.write(
                "index.html",
                fs::read_to_string(o.path().join("index.html")).unwrap().replace("href=\"diacraft.webmanifest\"", "href=\"manifest.json\""),
            )
        },
        "links the manifest `manifest.json`",
    );
    refuses(|o| fs::remove_file(o.path().join("diacraft.webmanifest")).unwrap(), "`diacraft.webmanifest` names a file the build did not make");
    // What it says.
    for (from, to, diagnostic) in [
        ("\"display\": \"standalone\"", "\"display\": \"browser\"", "must be `standalone`"),
        ("\"display\": \"standalone\"", "\"display\": \"minimal-ui\"", "must be `standalone`"),
        ("\"start_url\": \"/\"", "\"start_url\": \"/app/\"", "`start_url` is `/app/`"),
        ("\"scope\": \"/\"", "\"scope\": \"/app/\"", "`scope` is `/app/`"),
        ("\"id\": \"/\"", "\"id\": \"/x\"", "`id` is `/x`"),
        ("\"start_url\": \"/\"", "\"start_url\": \"https://diacraft.invalid/\"", "`start_url` is `https://diacraft.invalid/`"),
        ("\"name\": \"DiaCraft\"", "\"name\": \"\"", "`name` must be text"),
        ("\"theme_color\": \"#202020\"", "\"theme_color\": \"gray\"", "#rrggbb"),
        ("\"background_color\": \"#181818\"", "\"background_color\": \"#18181\"", "#rrggbb"),
        ("\"purpose\": \"any\" },\n    { \"src\": \"icon-512", "\"purpose\": \"monochrome\" },\n    { \"src\": \"icon-512", "purpose"),
        ("icon-192.png\", \"sizes\": \"192x192\"", "icon-192.png\", \"sizes\": \"192x193\"", "`WxW`"),
        (
            "\"type\": \"image/png\", \"purpose\": \"any\" },\n    { \"src\"",
            "\"type\": \"image/webp\", \"purpose\": \"any\" },\n    { \"src\"",
            "image/png",
        ),
        ("\"src\": \"icon-192.png\"", "\"src\": \"gone.png\"", "not a file beside the page"),
        ("\"src\": \"icon-192.png\"", "\"src\": \"../icon.png\"", "not a file beside the page"),
        ("\"src\": \"icon-192.png\"", "\"src\": \"https://cdn.invalid/i.png\"", "not a file beside the page"),
        ("\"icons\": [", "\"serviceworker\": {\"src\": \"sw.js\"},\n  \"icons\": [", "`serviceworker`"),
        ("\"icons\": [", "\"related_applications\": [],\n  \"icons\": [", "`related_applications`"),
        ("\"icons\": [", "\"shortcuts\": [],\n  \"icons\": [", "`shortcuts`"),
    ] {
        assert!(MANIFEST.contains(from), "{from}");
        let changed = MANIFEST.replace(from, to);
        let out = out_dir();
        out.write("diacraft.webmanifest", &changed);
        let error = app::check(out.path()).unwrap_err();
        assert!(error.contains(diagnostic), "{from} -> {to}: expected {diagnostic:?} in {error:?}");
    }
    // The icons are the sizes they say, and both sizes an installable page needs are there.
    refuses(|o| o.write("icon-192.png", png(190, 190)), "says it is 192 by 192");
    refuses(|o| o.write("icon-512.png", b"not a png at all, only text, and long enough to have a header"), "says it is 512 by 512");
    let only_192 =
        MANIFEST.replace(",\n    { \"src\": \"icon-512.png\", \"sizes\": \"512x512\", \"type\": \"image/png\", \"purpose\": \"any\" }", "");
    assert_ne!(only_192, MANIFEST);
    let out = out_dir();
    out.write("diacraft.webmanifest", only_192);
    assert!(app::check(out.path()).unwrap_err().contains("no icon of 512 by 512"));
    // Not JSON, not an object, and a name the site does not use.
    refuses(|o| o.write("diacraft.webmanifest", "{ not json"), "it is not JSON");
    refuses(|o| o.write("diacraft.webmanifest", "[]"), "not a JSON object");
    // `manifest.json` is the module's, and another web app manifest beside it is refused (the check lists the kinds of file the app uses).
    refuses(|o| o.write("other.webmanifest", MANIFEST), "a file of a kind the app does not use");
}

#[test]
fn a_png_header_gives_its_size_and_nothing_else_does() {
    assert_eq!(super::web_manifest::png_size(&png(192, 512)), Some((192, 512)));
    assert_eq!(super::web_manifest::png_size(&png(1, 1)), Some((1, 1)));
    assert_eq!(super::web_manifest::png_size(b""), None);
    assert_eq!(super::web_manifest::png_size(&png(192, 192)[..23]), None);
    let mut bad = png(192, 192);
    bad[12] = b'X';
    assert_eq!(super::web_manifest::png_size(&bad), None);
}

/// The strings below are text the check looks for in a script: they are written into files and never run.
#[test]
fn the_check_fails_on_a_script_that_reaches_the_network_or_evaluates_text() {
    for (banned, word) in [
        ("new XMLHttpRequest()", "XMLHttpRequest"),
        ("new WebSocket('x')", "WebSocket"),
        ("navigator.sendBeacon('x')", "sendBeacon"),
        ("new EventSource('x')", "EventSource"),
        ("new Function('x')", "new Function"),
        ("eval('x')", "eval("),
        ("importScripts('x')", "importScripts"),
    ] {
        refuses(|o| o.write(&glue(o), format!("{GLUE}{banned};\n")), word);
    }
    refuses(|o| o.write(&glue(o), format!("{GLUE}const u = 'https://example.invalid/x';\n")), "loads nothing from another site");
    refuses(|o| o.write(&glue(o), format!("{GLUE}const u = '//example.invalid/x';\n")), "loads nothing from another site");
    refuses(|o| o.write(&glue(o), format!("{GLUE}import './missing.js';\n")), "missing.js");
    refuses(|o| o.write(&glue(o), format!("{GLUE}import '../up.js';\n")), "not a file of the app");
    refuses(|o| o.write(&css(o), "@import url('https://example.invalid/a.css');"), "app/page-");
    refuses(|o| o.write(&css(o), "body { background: url(other.png) }"), "other.png");
}

#[test]
fn the_check_fails_on_a_module_that_has_a_shared_memory_and_the_build_on_one_that_is_not_a_module() {
    let out = out_dir_with(&module(&[(5, vec![1, 0x03, 1, 2])]));
    assert!(app::check(out.path()).unwrap_err().contains("shared memory"));
    let out = out_dir_with(&[EMPTY_WASM, &[5, 0x7f, 1]].concat());
    assert!(app::check(out.path()).unwrap_err().contains("could not be read"));
    // A module that is an error page saved under its name does not get as far as being cut.
    let root = repo();
    root.write(&format!("dist/web/{TRUNK_WASM}"), "<html>Not found</html>");
    let error = app::assemble(root.path(), &root.path().join("dist/web"), &root.path().join("out")).unwrap_err();
    assert!(error.contains("not a WebAssembly module"), "{error}");
}

/// The module is the program: it carries the namespace addresses and content types of the file formats it reads, and the licence texts of its libraries. Its
/// text is not scanned for names (the pages, the script and the style are).
#[test]
fn a_module_that_carries_the_names_of_the_formats_it_reads_passes() {
    let text = b"http://schemas.microsoft.com/visio/2010/relationships/ application/vnd.ms-visio.page+xml Copyright (c) The Example Authors";
    let out = out_dir_with(&[EMPTY_WASM, custom(text).as_slice()].concat());
    app::check(out.path()).unwrap();
    // The pages' own text still is: the script of the app, here.
    out.write("start.js", "import init from './diacraft-web-abc.js'; init(); // Microsoft");
    assert!(app::check(out.path()).unwrap_err().contains("banned name microsoft"));
}

#[test]
fn the_check_holds_each_file_to_the_budget_below_the_limit_of_pages_and_each_part_to_its_own() {
    // A hole of the size, not megabytes written: the size is what is looked at first.
    let sized = |name: &'static str, bytes: u64| {
        move |o: &Dir| {
            let file = fs::File::create(o.path().join(name)).unwrap();
            file.set_len(bytes).unwrap();
        }
    };
    // A file that is not a part: the budget of 4 MiB.
    let icon: &'static str = Box::leak(icon_out().into_boxed_str());
    refuses(sized(icon, app::FILE_BUDGET + 1), "over the budget, 4.00 MiB");
    refuses(sized(icon, app::PAGES_FILE_LIMIT + 1), "Cloudflare Pages allows a file");
    // A part: 8 MiB, which is more than the budget of a file that is not one.
    let out = out_dir();
    let part = fs::read_dir(out.path().join("parts")).unwrap().next().unwrap().unwrap().file_name().into_string().unwrap();
    let name: &'static str = Box::leak(format!("parts/{part}").into_boxed_str());
    refuses(sized(name, super::parts::PART_LIMIT + 1), "over the limit for a part of the module, 8.00 MiB");
    refuses(sized(name, super::parts::PART_LIMIT), "bytes on disk");
}

#[test]
fn the_check_refuses_names_kinds_and_headers_it_does_not_expect() {
    refuses(|o| o.write("diacraft-web-abc.bin", "x"), "a file of a kind the app does not use");
    refuses(|o| o.write("notes.txt", "x"), "a file of a kind the app does not use");
    refuses(|o| o.write("other.json", "{}"), "a file of a kind the app does not use");
    refuses(|o| o.write("other.wasm", EMPTY_WASM), "a WebAssembly file outside parts/");
    refuses(|o| o.write("parts/notes.txt", "x"), "a file of a kind the app does not use");
    refuses(|o| o.write("index.html.map", "x"), "a file of a kind the app does not use");
    // The pages' headers in the app's folder: the app would run under the policy that stops WebAssembly.
    refuses(|o| o.write("_headers", PAGES_HEADERS), "script-src");
    refuses(|o| o.write("_headers", HEADERS.replace("'wasm-unsafe-eval'", "'unsafe-eval'")), "script-src");
    refuses(|o| o.write("_headers", HEADERS.replace("/*\n", "/app/*\n")), "a rule for /app/*");
    refuses(|o| fs::remove_file(o.path().join("_headers")).unwrap(), "_headers");
    // Too many files: the folder is a handful (the finished one here has fifteen), and 64 is all it may hold.
    let out = out_dir();
    for n in 0..49 {
        out.write(&format!("extra{n}.png"), "x");
    }
    app::check(out.path()).unwrap();
    out.write("extra49.png", "x");
    assert!(app::check(out.path()).unwrap_err().contains("more than 64 files"));
}

#[cfg(unix)]
#[test]
fn a_link_in_the_apps_folder_is_refused() {
    refuses(|o| std::os::unix::fs::symlink("index.html", o.path().join("link.png")).unwrap(), "only regular files");
    refuses(|o| std::os::unix::fs::symlink("..", o.path().join("snippets/up")).unwrap(), "only regular files");
}

// ------------------------------------------------------------------------------------------------------------------------------ assembling

/// A repository root with a `site/` folder (the pages) and a finished Trunk build in `dist/web`.
pub(super) fn repo() -> Dir {
    let root = Dir::new("repo");
    root.write("site/index.html", PAGE);
    root.write("site/_headers", PAGES_HEADERS);
    root.write("site/guides/one.html", GUIDE);
    root.write("site/assets/social.jpg", "jpg");
    root.write("dist/web/index.html", TRUNK_PAGE);
    root.write(&format!("dist/web/{TRUNK_ICON}"), b"png");
    root.write("dist/web/diacraft.webmanifest", MANIFEST);
    root.write("dist/web/icon-192.png", png(192, 192));
    root.write("dist/web/icon-512.png", png(512, 512));
    root.write(&format!("dist/web/{TRUNK_CSS}"), "html { margin: 0 }\n");
    root.write(&format!("dist/web/{TRUNK_GLUE}"), GLUE);
    root.write(&format!("dist/web/{TRUNK_WASM}"), EMPTY_WASM);
    root.write("dist/web/snippets/eframe-1/inline0.js", "export const a = 1;\n");
    root.write("dist/web/.trunk-lock", "");
    // What the repository gives the app besides Trunk's build: the loader and the data the module does not carry.
    root.write("apps/diacraft-web/loader.js", LOADER);
    root.write("apps/diacraft-web/_headers", HEADERS);
    root.write("assets/branding/diacraft-splash.png", b"splash picture");
    root.write("assets/branding/diacraft-cover.png", b"cover picture");
    root.write("THIRD-PARTY-LICENSES-WEB.txt", "DiaCraft: third-party licences (the web build)\n");
    root
}

/// The web app manifest of the app, as the repository has it.
pub(super) const MANIFEST: &str = r##"{
  "id": "/",
  "name": "DiaCraft",
  "short_name": "DiaCraft",
  "description": "Open-source diagramming in the browser.",
  "start_url": "/",
  "scope": "/",
  "display": "standalone",
  "background_color": "#181818",
  "theme_color": "#202020",
  "icons": [
    { "src": "icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
    { "src": "icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" }
  ]
}
"##;

/// The head of a PNG file of `w` by `h` pixels: the signature and the header chunk, which is all the check reads.
pub(super) fn png(w: u32, h: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(b"IHDR");
    bytes.extend(w.to_be_bytes());
    bytes.extend(h.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes
}

/// A loader as small as the check needs: it names the manifest and starts the program from a stream.
pub(super) const LOADER: &str = "// the loader\nconst manifest = await (await fetch('./manifest.json')).json();\nconst glue = await import(new URL(manifest.glue, location.href).href);\nawait glue.default({ module_or_path: new Response(new ReadableStream({}), { headers: { 'Content-Type': 'application/wasm' } }) });\n";

pub(super) fn assemble(root: &Dir, out: &str) -> Result<(), String> {
    app::assemble(root.path(), &root.path().join("dist/web"), &root.path().join(out))
}

/// The settings the pages of [`repo`] are made for: their own address, and the app's at the address of a site of its own.
fn settings(app_url: &str) -> super::settings::Settings {
    super::settings::Settings {
        domain: "https://example.invalid".into(),
        app_url: app_url.into(),
        repository: "owner/drawing".into(),
        linux_asset: "l".into(),
        windows_asset: "w".into(),
        macos_asset: "m".into(),
    }
}

#[test]
fn assembling_puts_the_build_at_the_root_of_the_apps_folder_with_its_script_in_a_file_and_its_own_headers() {
    let root = repo();
    assemble(&root, "dist/app").unwrap();
    let out = root.path().join("dist/app");
    let page = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(page.contains("<script type=\"module\" src=\"./start.js\"></script>") && !page.contains("import init"), "{page}");
    // The headers are the app's own, as the repository has them; nothing of the pages is in the folder.
    assert_eq!(fs::read_to_string(out.join("_headers")).unwrap(), HEADERS);
    assert!(!out.join("guides").exists() && !out.join("app").exists(), "the pages are another site");
    // The start script is the repository's loader, and not the text Trunk made; the module is in parts, and the whole file is not in the folder.
    assert_eq!(fs::read_to_string(out.join("start.js")).unwrap(), LOADER);
    assert!(!out.join(TRUNK_WASM).exists());
    let parts: Vec<_> = fs::read_dir(out.join("parts")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(parts.len(), 1, "{parts:?}");
    assert_eq!(fs::read(out.join("parts").join(&parts[0])).unwrap(), EMPTY_WASM);
    assert!(!page.contains("_bg.wasm\""), "Trunk's preload of the whole module is gone: {page}");
    assert!(page.contains("<link rel=\"preload\" href=\"./manifest.json\" as=\"fetch\" crossorigin=\"anonymous\">"), "{page}");
    assert!(page.contains(&format!("href=\"./parts/{}\" as=\"fetch\"", parts[0])), "{page}");
    assert!(page.contains(&format!("<link rel=\"modulepreload\" href=\"./{}\"", glue_out())), "the glue's preload stays, under its new name: {page}");
    assert!(out.join("snippets/eframe-1/inline0.js").is_file());
    assert!(!out.join(".trunk-lock").exists(), "Trunk's lock file is not part of the site");
    // What it made passes the check, and the site folder and the build are as they were.
    app::check(&out).unwrap();
    assert!(!root.path().join("site/app").exists());
    assert_eq!(fs::read_to_string(root.path().join("dist/web/index.html")).unwrap(), TRUNK_PAGE);
}

#[test]
fn the_pages_folder_is_the_site_as_it_is_and_a_previews_links_to_the_app_are_the_previews() {
    let root = repo();
    let site = root.path().join("site");
    // The setting's own address: the folder is `site/`, byte for byte, and has no app in it.
    app::assemble_pages(root.path(), &site, &root.path().join("dist/site"), &settings("https://app.example.invalid")).unwrap();
    let out = root.path().join("dist/site");
    assert_eq!(fs::read_to_string(out.join("index.html")).unwrap(), PAGE);
    assert_eq!(fs::read_to_string(out.join("_headers")).unwrap(), PAGES_HEADERS);
    assert_eq!(fs::read_to_string(out.join("guides/one.html")).unwrap(), GUIDE);
    assert!(!out.join("app").exists(), "no app in the pages' folder");
    // A preview: the copy's links to the app are the preview's, and the pages in `site/` are not touched.
    let preview = settings("http://127.0.0.1:8100");
    app::assemble_pages(root.path(), &site, &root.path().join("dist/preview"), &preview).unwrap();
    let copy = fs::read_to_string(root.path().join("dist/preview/index.html")).unwrap();
    assert!(copy.contains("<a data-app=\"\" href=\"http://127.0.0.1:8100/\">Open the app</a>"), "{copy}");
    assert_eq!(fs::read_to_string(site.join("index.html")).unwrap(), PAGE);
    // The copy passes the check of the pages, for the preview's address and for no other.
    super::check_pages(&root.path().join("dist/preview"), &preview).unwrap();
    let error = super::check_pages(&root.path().join("dist/preview"), &settings("https://app.example.invalid")).unwrap_err();
    assert!(error.contains("unapproved") || error.contains("stale"), "{error}");
}

#[test]
fn the_two_folders_are_apart_and_a_run_makes_both() {
    let root = repo();
    root.write("site/site.json", r#"{"domain":"https://example.invalid","app_url":"https://app.example.invalid","repository":"owner/drawing","linux_asset":"l","windows_asset":"w","macos_asset":"m"}"#);
    let site = root.path().join("site");
    // The pages as `cargo xtask site --write` leaves them: the sitemap and the robots file are made.
    super::settings::apply(&site, &settings("https://app.example.invalid"), true).unwrap();
    let web = |out: &str, app_out: &str| Web {
        dist: Some(root.path().join("dist/web")),
        out: Some(root.path().join(out)),
        app_out: Some(root.path().join(app_out)),
        app_url: None,
    };
    let settings = settings("https://app.example.invalid");
    // The same folder, or one inside the other, would deploy the one site's files with the other's.
    for (out, app_out) in [("dist/x", "dist/x"), ("dist/x", "dist/x/app"), ("dist/x/pages", "dist/x"), ("dist/a/../x", "dist/x")] {
        let error = app::run(&site, &settings, None, &web(out, app_out)).unwrap_err();
        assert!(error.contains("two sites") && error.contains("neither may be the other or inside it"), "{out} {app_out}: {error}");
    }
    app::run(&site, &settings, None, &web("dist/site", "dist/app")).unwrap();
    assert!(root.path().join("dist/site/index.html").is_file() && !root.path().join("dist/site/app").exists());
    assert!(root.path().join("dist/app/index.html").is_file() && !root.path().join("dist/app/guides").exists());
    // A preview address is the one the copy of the pages has, and the app's folder has nothing of it.
    app::run(&site, &settings, Some("http://127.0.0.1:8100".into()), &web("dist/site2", "dist/app2")).unwrap();
    assert!(fs::read_to_string(root.path().join("dist/site2/index.html")).unwrap().contains("href=\"http://127.0.0.1:8100/\""));
    assert!(
        fs::read_to_string(root.path().join("dist/site2/site.json")).unwrap().contains("\"app_url\": \"http://127.0.0.1:8100\""),
        "the copy's settings say what its pages say"
    );
    assert!(fs::read_to_string(site.join("site.json")).unwrap().contains("https://app.example.invalid"), "site/ is as it was");
    assert!(!fs::read_to_string(root.path().join("dist/app2/index.html")).unwrap().contains("127.0.0.1"));
}

#[test]
fn a_run_takes_the_previews_address_from_the_option_and_then_from_the_environment_and_refuses_one_that_is_not_for_a_preview() {
    let root = repo();
    root.write("site/site.json", r#"{"domain":"https://example.invalid","app_url":"https://app.example.invalid","repository":"owner/drawing","linux_asset":"l","windows_asset":"w","macos_asset":"m"}"#);
    let site = root.path().join("site");
    // The pages as `cargo xtask site --write` leaves them: the sitemap and the robots file are made.
    super::settings::apply(&site, &settings("https://app.example.invalid"), true).unwrap();
    let dist = root.path().join("dist/web");
    let line = |out: &str, app_out: &str, extra: &[&str]| {
        let path = |name: &str| root.path().join(name).to_str().unwrap().to_owned();
        let mut args: Vec<String> =
            ["--web", "--dist", dist.to_str().unwrap(), "--out", &path(out), "--app-out", &path(app_out)].map(str::to_owned).to_vec();
        args.extend(extra.iter().map(|a| (*a).to_owned()));
        args
    };
    let run = |args: &[String], env: Option<&str>| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        super::run_with(&site, &args, env)
    };
    let linked = |out: &str| fs::read_to_string(root.path().join(out).join("index.html")).unwrap();
    // The option wins over the environment; the environment is used when there is no option; and with neither the pages name the setting's address.
    run(&line("dist/s1", "dist/a1", &["--app-url", "http://127.0.0.1:8201/"]), Some("http://127.0.0.1:9999")).unwrap();
    assert!(linked("dist/s1").contains("href=\"http://127.0.0.1:8201/\""));
    run(&line("dist/s2", "dist/a2", &[]), Some("http://localhost:8202")).unwrap();
    assert!(linked("dist/s2").contains("href=\"http://localhost:8202/\""));
    run(&line("dist/s3", "dist/a3", &[]), Some("")).unwrap();
    assert!(linked("dist/s3").contains("href=\"https://app.example.invalid/\""));
    run(&line("dist/s4", "dist/a4", &[]), None).unwrap();
    assert!(linked("dist/s4").contains("href=\"https://app.example.invalid/\""));
    // A site that is not the person's own is no preview.
    for bad in ["http://app.example.invalid", "http://127.0.0.1", "ftp://127.0.0.1:80"] {
        let error = run(&line("dist/s5", "dist/a5", &["--app-url", bad]), None).unwrap_err();
        assert!(error.contains(&format!("`{bad}`")), "{bad}: {error}");
    }
    // The environment is read as the option is.
    let error = run(&line("dist/s5", "dist/a5", &[]), Some("http://evil.example:80")).unwrap_err();
    assert!(error.contains("http://evil.example:80"), "{error}");
}

#[test]
fn assembling_again_clears_what_a_run_made_before_and_leaves_a_folder_that_is_somebodys_alone() {
    let root = repo();
    assemble(&root, "dist/app").unwrap();
    root.write("dist/app/stale.txt", "old");
    assemble(&root, "dist/app").unwrap();
    assert!(!root.path().join("dist/app/stale.txt").exists(), "the folder this command made is cleared");
    // A folder outside `dist/` that has something in it is not touched.
    root.write("mine/keep.txt", "mine");
    let error = assemble(&root, "mine").unwrap_err();
    assert!(error.contains("not empty"), "{error}");
    assert_eq!(fs::read_to_string(root.path().join("mine/keep.txt")).unwrap(), "mine");
    // An empty one is used, and a file in the way is an error.
    fs::create_dir(root.path().join("empty")).unwrap();
    assemble(&root, "empty").unwrap();
    assert!(root.path().join("empty/index.html").is_file());
    root.write("file", "x");
    assert!(assemble(&root, "file").unwrap_err().contains("not a folder"));
    // `dist` itself is not cleared by a run that was told to make it.
    assert!(assemble(&root, "dist").unwrap_err().contains("not empty"));
}

#[test]
fn the_pages_refuse_an_app_kept_in_them_and_a_build_with_no_page_is_refused() {
    let root = repo();
    root.write("site/app/index.html", "x");
    let site = root.path().join("site");
    let error = app::assemble_pages(root.path(), &site, &root.path().join("dist/site"), &settings("https://app.example.invalid")).unwrap_err();
    assert!(error.contains("the pages hold no app"), "{error}");
    fs::remove_dir_all(root.path().join("site/app")).unwrap();
    fs::remove_file(root.path().join("dist/web/index.html")).unwrap();
    let error = assemble(&root, "dist/app").unwrap_err();
    assert!(error.contains("no index.html") && error.contains("trunk build --release"), "{error}");
    // The command refuses the app kept in the pages before it reads anything else.
    root.write("site/app/index.html", "x");
    let error = super::run(&root.path().join("site"), &[]).unwrap_err();
    assert!(error.contains("the pages hold no app"), "{error}");
}

#[test]
fn a_build_the_page_of_which_already_has_a_start_file_is_refused_and_a_deep_tree_too() {
    let root = repo();
    root.write("dist/web/start.js", "x");
    assert!(assemble(&root, "dist/app").unwrap_err().contains("already has a file of that name"));
    // The same for the app's headers: the build is not what makes them.
    let root = repo();
    root.write("dist/web/_headers", "x");
    assert!(assemble(&root, "dist/app").unwrap_err().contains("already has a file of that name"));
    // Two modules, or none, or two start scripts, are not a build this tool knows how to cut.
    let root = repo();
    root.write("dist/web/other_bg.wasm", EMPTY_WASM);
    assert!(assemble(&root, "dist/app").unwrap_err().contains("expected one .wasm file in the build, found 2"));
    let root = repo();
    root.write("dist/web/second.js", GLUE);
    assert!(assemble(&root, "dist/app").unwrap_err().contains("expected one .js file in the build, found 2"));
    let root = repo();
    let deep = "a/b/c/d/e/f/g/h/i/j";
    root.write(&format!("dist/web/{deep}/x.js"), "x");
    assert!(assemble(&root, "dist/app").unwrap_err().contains("nested more than 8"));
}

// ------------------------------------------------------------------------------------------------------------------ the names Trunk made

#[test]
fn the_files_trunk_hashed_are_named_again_by_the_sha256_of_their_bytes_and_the_page_follows() {
    let root = repo();
    assemble(&root, "dist/app").unwrap();
    let app = root.path().join("dist/app");
    let page = fs::read_to_string(app.join("index.html")).unwrap();
    for (old, stem, ext, bytes) in [
        (TRUNK_ICON, "diacraft-icon-256", "png", b"png".as_slice()),
        (TRUNK_CSS, "page", "css", b"html { margin: 0 }\n".as_slice()),
        (TRUNK_GLUE, "diacraft-web", "js", GLUE.as_bytes()),
    ] {
        let new = format!("{stem}-{}.{ext}", &parts::sha256(bytes)[..16]);
        assert!(app.join(&new).is_file(), "{new} is in the app's folder");
        assert!(!app.join(old).exists(), "{old} was renamed, not copied");
        assert!(page.contains(&format!("./{new}")) && !page.contains(old), "the page names {new} and not {old}: {page}");
        assert_eq!(fs::read(app.join(&new)).unwrap(), bytes, "the content is the same");
    }
    // What Trunk wrote in the tags beside the name stays: the integrity of the file is of its bytes, and those did not change.
    assert!(page.contains("integrity=\"sha384-x\"") && page.contains("integrity=\"sha384-y\"") && page.contains("integrity=\"sha384-z\""), "{page}");
    // The manifest and the check follow: the glue is under its new name, and the finished folder passes.
    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(app.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["glue"], glue_out());
    app::check(&root.path().join("dist/app")).unwrap();
    // The files that are not hashed keep their names.
    for plain in ["start.js", "index.html", "diacraft.webmanifest", "icon-192.png", "icon-512.png", "manifest.json"] {
        assert!(app.join(plain).is_file(), "{plain}");
    }
}

#[test]
fn a_file_that_is_named_by_its_own_hash_already_is_left_as_it_is() {
    let root = repo();
    let name = format!("page-{}.css", &parts::sha256(b"html { margin: 0 }\n")[..16]);
    fs::rename(root.path().join("dist/web").join(TRUNK_CSS), root.path().join("dist/web").join(&name)).unwrap();
    root.write("dist/web/index.html", TRUNK_PAGE.replace(TRUNK_CSS, &name));
    assemble(&root, "dist/app").unwrap();
    assert!(root.path().join("dist/app").join(&name).is_file());
    app::check(&root.path().join("dist/app")).unwrap();
}

#[test]
fn a_hashed_name_of_trunk_is_twelve_to_sixteen_lower_case_digits_after_a_dash_and_a_kind_it_makes() {
    assert_eq!(app::trunk_hashed("page-00112233445566bb.css"), Some(("page", "css")));
    assert_eq!(app::trunk_hashed("diacraft-web-00112233445566cc.js"), Some(("diacraft-web", "js")));
    assert_eq!(app::trunk_hashed("diacraft-icon-256-00112233445566aa.png"), Some(("diacraft-icon-256", "png")));
    // Trunk writes a hash of the glue without its leading zeros: fifteen digits, or twelve at the least here.
    assert_eq!(app::trunk_hashed("diacraft-web-12233445566cc77.js"), Some(("diacraft-web", "js")));
    assert_eq!(app::trunk_hashed("diacraft-web-0123456789ab.js"), Some(("diacraft-web", "js")));
    for no in [
        "icon-192.png",
        "icon-512.png",
        "start.js",
        "index.html",
        "manifest.json",
        "diacraft.webmanifest",
        "page-0123456789a.css",
        "page-00112233445566bbb.css",
        "page-00112233445566BB.css",
        "page-00112233445566bb.map",
        "page-00112233445566bb",
        "-00112233445566bb.css",
        "diacraft-web-00112233445566dd_bg.wasm",
        "page-00112233445566bb.css.map",
    ] {
        assert_eq!(app::trunk_hashed(no), None, "{no}");
    }
}

#[test]
fn a_hashed_file_the_page_does_not_name_two_that_come_to_one_name_and_too_many_are_refused() {
    // A hashed file the page never links: Trunk's output changed, and a file would be named and cached that nothing asks for.
    let root = repo();
    root.write("dist/web/other-00112233445566ee.png", b"x");
    let error = assemble(&root, "dist/app").unwrap_err();
    assert!(error.contains("other-00112233445566ee.png") && error.contains("page does not link"), "{error}");
    // Two files with the same stem and content: one name.
    let root = repo();
    root.write("dist/web/page-00112233445566ee.css", "html { margin: 0 }\n");
    root.write("dist/web/index.html", TRUNK_PAGE.replace("</head>", "<link rel=\"stylesheet\" href=\"./page-00112233445566ee.css\"/></head>"));
    let error = assemble(&root, "dist/app").unwrap_err();
    assert!(error.contains("same name"), "{error}");
    // More than eight is more than Trunk makes here.
    let root = repo();
    let mut page = TRUNK_PAGE.to_owned();
    for n in 0..9 {
        root.write(&format!("dist/web/extra-00112233445566{n:02x}.png"), format!("{n}"));
        page = page.replace("</head>", &format!("<link rel=\"icon\" href=\"./extra-00112233445566{n:02x}.png\"/></head>"));
    }
    root.write("dist/web/index.html", page);
    assert!(assemble(&root, "dist/app").unwrap_err().contains("more than 8"));
}

#[test]
fn a_finished_folder_reports_what_is_kept_for_good_and_what_is_asked_about() {
    let out = out_dir();
    let report = app::check(out.path()).unwrap();
    // The module's one part, the three assets, the glue, the style and the icon are named by their hash; the page, the loader, the manifests, their icons and the snippet are not.
    assert_eq!(report.caching, super::cache::Summary { kept: 7, revalidated: 7, pages: 1 }, "{report:?}");
    assert!(
        report.lines().iter().any(|l| l.contains("7 files named by their own hash are kept for a year, 7 are asked about")),
        "{:?}",
        report.lines()
    );
}

#[test]
fn the_check_refuses_a_folder_whose_headers_would_keep_what_a_release_must_replace() {
    // The loader kept for a year: a release would not reach a visitor who has it.
    // (The file of headers holds no other route, so the check of the file is the first to say so; the check of the folder says it of the file.)
    let broad = format!("{HEADERS}\n/start.js\n  Cache-Control: public, max-age=31536000, immutable\n");
    let out = out_dir();
    out.write("_headers", &broad);
    let error = app::check(out.path()).unwrap_err();
    assert!(error.contains("a rule for /start.js"), "{error}");
    // A rule that leaves the glue, the style or the icon asked about on every visit.
    for route in ["/diacraft-web-*.js", "/page-*.css", "/diacraft-icon-256-*.png"] {
        let missing = HEADERS.replace(&format!("\n\n{route}\n  Cache-Control: public, max-age=31536000, immutable"), "");
        assert_ne!(missing, HEADERS, "{route}");
        let out = out_dir();
        out.write("_headers", &missing);
        let error = app::check(out.path()).unwrap_err();
        assert!(error.contains(route) && error.contains("must be cached for good"), "{route}: {error}");
    }
    // The app loses its policy: a page the rules do not reach.
    let nothing = HEADERS.replace("/*\n  Content-Security-Policy", "/nothing/*\n  X-Other");
    let out = out_dir();
    out.write("_headers", &nothing);
    let error = app::check(out.path()).unwrap_err();
    assert!(error.contains("a rule for /nothing/*") || error.contains("policy"), "{error}");
}
