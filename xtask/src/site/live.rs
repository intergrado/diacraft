//! The check after a deploy: `cargo xtask deploy-check <BASE_URL> [<BASE_URL>]` asks each deployed site what it sends, and compares that with what `_headers` says it should.
//! The website is two sites, the pages (`https://diacraft.dev`) and the web app at the root of its own (`https://diacraft.app`), each a Pages project of its own: the command takes the address of one
//! or of both, and checks each as a site (the two have no file in common, and the same rules).
//!
//! **Why a command of xtask and not a script.** The project has no shell scripts and no handwritten JavaScript: its tools are Rust, they are tested with the rest, and they share what they
//! know. This one shares `cache::name_hash`, the shape of a name that is a hash of its own file, with the offline check, so what the offline check holds to `_headers` and what this holds
//! to the deployed site are one rule. It runs `curl` (as `site --web` runs `trunk`), which every machine of the project has, and so needs no HTTP or TLS library of its own.
//!
//! **What it fetches.** The start page of the site (`/`: the home page of the pages, the app's page of the app), the pages of the site it links to (pictures are on those too), and every file they load: the style, the pictures (`<img>`,
//! `srcset`, the share picture, the style's `url()`), the web manifests and their icons, the app's script, its glue, its snippets and, from the module's `manifest.json`, the parts of
//! the module and the data. A link to another site (the pages' to the app) is not followed: that is the other site's check. Each is fetched once with
//! `Accept-Encoding: br, gzip`, as a browser asks (the answer is not decoded: what comes is what a browser is sent), and again with `If-None-Match` and the `ETag` it was given.
//!
//! **What Pages does to a link with an extension.** The pages link each other as `/privacy.html`, so that they also work on a plain file server, and Cloudflare Pages answers such a link
//! with a 308 to the same address without the extension (`/privacy`), which it serves. That answer is right: a page whose address ends in `.html` and that answers 308 to its own address without it
//! (`/index.html` to `/`, `/dir/index.html` to `/dir/`), as a path or as the site's own address, is not a failure; the check follows it once and checks the target as it checks any file, and finds
//! what the target loads. Any other 308, any other redirect and a redirect of a path that is not a page are answers that are not 200, as they were.
//!
//! **The registry's proof of the domain.** On the app's site (the start page that has the app's canvas) the check also asks for `/.well-known/mcp-registry-auth`, the file the official MCP
//! Registry fetches to see that the owner of the domain publishes the server `app.diacraft/diacraft`. Nothing links it, and the web app answers every path that has no file with its page, so a file that a
//! deploy left out (Cloudflare Pages might leave out a name that starts with a dot) would be an answer of `200` and HTML, which the registry refuses. It **fails** unless the answer is a `200` with no redirect,
//! a type of `text/plain`, and a body that is a record (`app::registry_record`).
//!
//! **What it says.** For each: the status, `Cache-Control`, `Content-Encoding`, `ETag`, whether the second request got `304`, and the bytes that came. It **fails** on a status that is not 200
//! and on a wrong `Cache-Control` (a file named by its hash must be `immutable` for a year; any other must not be kept at all) and **warns** when a text or WebAssembly file of 1 KiB
//! or more comes back uncompressed, when there is no `ETag` or the second request does not get `304`, and when a file that must be asked about says nothing about it.

use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{app, cache, links};

pub(super) const USAGE: &str = "usage: cargo xtask deploy-check <BASE_URL> [<BASE_URL>]   (the pages, then the app; each https://host, or http://host:port for a server of your own; no path)";

/// The most sites one run checks: the pages and the app.
const MAX_SITES: usize = 2;

/// The most files the check looks at, the most text it reads of one, and the most output it takes from one `curl`.
const MAX_URLS: usize = 200;
const MAX_BODY: u64 = 2 * 1024 * 1024;
const MAX_HEAD: u64 = 256 * 1024;
/// A file that comes back as is, and is smaller, is not worth warning of.
const COMPRESS_FROM: u64 = 1024;

/// One response: the status, its headers (names in lower case, in the order they came), the bytes that came (what `curl` counted, whatever it kept) and the body when it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Response {
    pub(super) status: u16,
    pub(super) headers: Vec<(String, String)>,
    pub(super) size: u64,
    pub(super) body: Vec<u8>,
}

impl Response {
    /// The first header of that name (lower case).
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// How a path of the site is fetched: `if_none_match` is an `ETag` to send, and `body` says whether the text is wanted (decoded) or only the head and the size (as sent, with the
/// compressed forms asked for).
pub(super) trait Fetch {
    fn get(&self, path: &str, if_none_match: Option<&str>, body: bool) -> Result<Response, String>;

    /// The address of the site itself, `https://host` (or `http://host:port`), with no slash at the end: a redirect to it is a redirect within the site. A fetcher that does not know it says
    /// so, and then only a path is taken for the site's own.
    fn origin(&self) -> Option<&str> {
        None
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ curl

/// `curl`, against one site.
pub(super) struct Curl {
    base: String,
}

/// `https://host` or `http://host:port`, with no path and nothing that could pass as an option or a second address.
pub(super) fn parse_base(text: &str) -> Result<String, String> {
    let (scheme, rest) = if let Some(rest) = text.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = text.strip_prefix("http://") {
        ("http", rest)
    } else {
        return Err(format!("`{text}` is not an address with https:// or http://\n{USAGE}"));
    };
    let host = rest.strip_suffix('/').unwrap_or(rest);
    if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':')) {
        return Err(format!("`{text}` must be a host and perhaps a port, and nothing else (no path, query, fragment or user)\n{USAGE}"));
    }
    Ok(format!("{scheme}://{host}"))
}

impl Curl {
    pub(super) fn new(base: &str) -> Result<Curl, String> {
        Ok(Curl { base: parse_base(base)? })
    }
}

/// The place `curl` puts a body nobody wants.
const SINK: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

impl Fetch for Curl {
    fn origin(&self) -> Option<&str> {
        Some(&self.base)
    }

    fn get(&self, path: &str, if_none_match: Option<&str>, body: bool) -> Result<Response, String> {
        if !path.starts_with('/') || path.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("`{path}` is not a path of the site"));
        }
        let url = format!("{}{path}", self.base);
        // `-q` first: no `.curlrc`. The head goes to the output with the body (when it is wanted), and the size `curl` counted after both.
        let mut args: Vec<String> =
            ["-q", "--silent", "--show-error", "--max-time", "90", "--connect-timeout", "20", "-D", "-", "-w", "\n@@size=%{size_download}\n"]
                .iter()
                .map(|a| (*a).to_owned())
                .collect();
        if body {
            args.extend(["--compressed".to_owned(), "-o".to_owned(), "-".to_owned()]);
        } else {
            args.extend(["-H".to_owned(), "Accept-Encoding: br, gzip".to_owned(), "-o".to_owned(), SINK.to_owned()]);
        }
        if let Some(etag) = if_none_match {
            if etag.chars().any(char::is_control) {
                return Err("an ETag with a control character in it".into());
            }
            args.extend(["-H".to_owned(), format!("If-None-Match: {etag}")]);
        }
        args.extend(["--url".to_owned(), url]);
        let mut child = Command::new("curl").args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "curl is not installed: this check runs it".to_owned()
            } else {
                format!("curl: failed to start: {e}")
            }
        })?;
        let (Some(mut stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err("curl: no output to read".into());
        };
        // What it says on stderr is read while the output is, so that neither fills its pipe and stops it.
        let said = std::thread::spawn(move || {
            let mut text = Vec::new();
            // A failed read of an error message leaves what was read: the message is only for the person.
            let _ = stderr.take(64 * 1024).read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        });
        let limit = if body { MAX_BODY + MAX_HEAD } else { MAX_HEAD };
        let mut out = Vec::new();
        let read = stdout.by_ref().take(limit + 1).read_to_end(&mut out);
        let too_much = out.len() as u64 > limit;
        if too_much || read.is_err() {
            // Nobody wants the rest: stop it, and say so. (A kill of a program that has ended already is no error to report.)
            let _ = child.kill();
        }
        let status = child.wait().map_err(|e| format!("curl: {e}"))?;
        let said = said.join().unwrap_or_default();
        if too_much {
            return Err(format!("{path}: more than {limit} bytes of output: the check reads text of at most {MAX_BODY}"));
        }
        read.map_err(|e| format!("curl: {e}"))?;
        if !status.success() {
            return Err(format!("curl failed ({status}) for {path}: {}", said.trim()));
        }
        parse_response(&out)
    }
}

/// The last place in `hay` where `needle` starts.
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|window| window == needle)
}

/// The end of a head: the first blank line, and where the next part begins.
fn head_end(bytes: &[u8]) -> Option<(usize, usize)> {
    let crlf = bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|at| (at, at + 4));
    let lf = bytes.windows(2).position(|w| w == b"\n\n").map(|at| (at, at + 2));
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// What `curl -D - -w "\n@@size=%{size_download}\n"` printed: the head of each response (informational ones too), the body of the last, and the size.
pub(super) fn parse_response(raw: &[u8]) -> Result<Response, String> {
    const MARK: &[u8] = b"\n@@size=";
    let at = rfind(raw, MARK).ok_or("curl printed no size line")?;
    let after = raw.get(at + MARK.len()..).ok_or("curl printed no size")?;
    let digits = String::from_utf8_lossy(after.split(|b| *b == b'\n').next().unwrap_or(&[])).trim().to_owned();
    let size: u64 = digits.parse().map_err(|_| format!("curl printed the size `{digits}`, which is no number"))?;
    let mut rest = raw.get(..at).ok_or("curl printed no response")?;
    loop {
        if !rest.starts_with(b"HTTP/") {
            return Err("curl printed no response head".into());
        }
        let (end, next) = head_end(rest).ok_or("the head of a response is not finished")?;
        let head = String::from_utf8_lossy(rest.get(..end).unwrap_or(&[])).into_owned();
        let after_head = rest.get(next..).unwrap_or(&[]);
        let mut lines = head.lines();
        let status_line = lines.next().unwrap_or("");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .filter(|code| (100..=599).contains(code))
            .ok_or_else(|| format!("`{status_line}` is not a status line"))?;
        let mut headers = Vec::new();
        for line in lines.filter(|l| !l.trim().is_empty()) {
            let (name, value) = line.split_once(':').ok_or_else(|| format!("`{line}` is not a header"))?;
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
        if status < 200 {
            rest = after_head;
            continue;
        }
        return Ok(Response { status, headers, size, body: after_head.to_vec() });
    }
}

// ------------------------------------------------------------------------------------------------------------------------ what to fetch

/// A reference of a page as a path of the site, or nothing for what is not a file of it: an empty one, a fragment, `data:` and the other schemes, one with a space or a backslash.
/// An address with a host (`https://host/a.png`, `//host/a.png`) is the same path on the site being checked: the page names its canonical domain, and the deploy answers at another.
pub(super) fn resolve_ref(page: &str, reference: &str) -> Option<String> {
    if reference.is_empty() || reference.len() > 2048 || reference.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return None;
    }
    let reference = reference.split(['#', '?']).next().unwrap_or("");
    let (absolute, path) = if let Some(rest) =
        reference.strip_prefix("https://").or_else(|| reference.strip_prefix("http://")).or_else(|| reference.strip_prefix("//"))
    {
        (true, rest.find('/').and_then(|at| rest.get(at..))?)
    } else if reference.split('/').next().is_some_and(|first| first.contains(':')) {
        return None;
    } else {
        (false, reference)
    };
    if path.is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    if !absolute && !path.starts_with('/') {
        let dir = page.rfind('/').and_then(|at| page.get(..=at)).unwrap_or("/");
        segments.extend(dir.split('/').filter(|s| !s.is_empty()));
    }
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            part => segments.push(part),
        }
    }
    let mut resolved = format!("/{}", segments.join("/"));
    if path.ends_with('/') && !segments.is_empty() {
        resolved.push('/');
    }
    Some(resolved)
}

/// What a page loads: the `href` of a stylesheet, icon, manifest or preload, the `src` of a script, picture or source, the pictures of a `srcset`, and the share picture of the
/// metadata. Not what it links to (`<a>`, the canonical address, an alternate), not a `data:` address or a fragment, and no address of another site except the share picture's.
pub(super) fn html_refs(html: &str) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    for piece in html.split('<').skip(1) {
        let tag = piece.split('>').next().unwrap_or("");
        let name = tag.split(|c: char| c.is_ascii_whitespace() || c == '/').next().unwrap_or("").to_ascii_lowercase();
        let mut found: Vec<&str> = Vec::new();
        match name.as_str() {
            "link" => {
                let rel = app::attribute(tag, "rel").unwrap_or("").to_ascii_lowercase();
                if ["stylesheet", "icon", "manifest", "preload"].iter().any(|kind| rel.contains(kind))
                    && let Some(href) = app::attribute(tag, "href")
                {
                    found.push(href);
                }
            }
            "script" => found.extend(app::attribute(tag, "src")),
            "img" | "source" => {
                found.extend(app::attribute(tag, "src"));
                if let Some(srcset) = app::attribute(tag, "srcset") {
                    found.extend(srcset.split(',').filter_map(|candidate| candidate.split_whitespace().next()));
                }
            }
            "meta" => {
                let key = app::attribute(tag, "property").or_else(|| app::attribute(tag, "name")).unwrap_or("").to_ascii_lowercase();
                if matches!(key.as_str(), "og:image" | "twitter:image") {
                    refs.extend(app::attribute(tag, "content").map(str::to_owned));
                }
            }
            _ => {}
        }
        for reference in found {
            let external = reference.starts_with("http://") || reference.starts_with("https://") || reference.starts_with("//");
            if !external && !reference.starts_with("data:") && !reference.starts_with('#') {
                refs.push(reference.to_owned());
            }
        }
    }
    refs
}

/// The pages a page links to (`<a href>`): the other pages of the site, which have pictures of their own. Only an address of the site itself that is a page (a folder, or `.html`).
pub(super) fn html_links(html: &str) -> Vec<String> {
    let mut links = Vec::new();
    for piece in html.split('<').skip(1) {
        let tag = piece.split('>').next().unwrap_or("");
        let name = tag.split(|c: char| c.is_ascii_whitespace() || c == '/').next().unwrap_or("");
        if !name.eq_ignore_ascii_case("a") {
            continue;
        }
        if let Some(href) = app::attribute(tag, "href")
            && !href.starts_with("http://")
            && !href.starts_with("https://")
            && !href.starts_with("//")
        {
            links.push(href.to_owned());
        }
    }
    links
}

/// The files a JSON manifest names: an icon's `src` (a web app manifest), the `glue` and the `file` of each part and asset (the module's own).
fn json_refs(value: &Value, depth: usize, refs: &mut Vec<String>) {
    if depth > 16 {
        return;
    }
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                match value {
                    Value::String(text) if matches!(key.as_str(), "src" | "glue" | "file") => refs.push(text.clone()),
                    other => json_refs(other, depth + 1, refs),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| json_refs(item, depth + 1, refs)),
        _ => {}
    }
}

/// A path that is a page: a folder or an `.html` file.
fn is_page(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with('/') || lower.ends_with(".html")
}

/// What a text file says to fetch next, by its kind: a page its files and its links to other pages (each as `(reference, is a link to a page)`), a style its `url()`s, a manifest its icons
/// or its parts.
fn discover(path: &str, text: &str) -> Vec<(String, bool)> {
    let lower = path.to_ascii_lowercase();
    let mut refs: Vec<(String, bool)> = Vec::new();
    if is_page(path) {
        refs.extend(html_refs(text).into_iter().map(|r| (r, false)));
        refs.extend(html_links(text).into_iter().map(|r| (r, true)));
    } else if lower.ends_with(".css") {
        refs.extend(links::css(text).unwrap_or_default().into_iter().map(|r| (r, false)));
    } else if (lower.ends_with(".json") || lower.ends_with(".webmanifest"))
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        let mut named = Vec::new();
        json_refs(&value, 0, &mut named);
        refs.extend(named.into_iter().map(|r| (r, false)));
    }
    refs
}

/// The kinds of file whose answers carry the references to the next.
fn has_references(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    ["/", ".html", ".css", ".json", ".webmanifest"].iter().any(|end| lower.ends_with(end))
}

// ----------------------------------------------------------------------------------------------------------------------------- what to say

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Level {
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    pub(super) level: Level,
    pub(super) text: String,
}

/// One URL: what it was sent with, what the second request got, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) path: String,
    pub(super) status: u16,
    pub(super) cache_control: Option<String>,
    pub(super) encoding: Option<String>,
    pub(super) etag: Option<String>,
    pub(super) size: u64,
    /// The status of the request that carried the `ETag`, when there was one.
    pub(super) conditional: Option<u16>,
    /// The path a 308 of Pages sent this page to (`/privacy.html` to `/privacy`), which is probed as a file of its own.
    pub(super) redirect: Option<String>,
    pub(super) findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) entries: Vec<Entry>,
}

impl Report {
    /// The files with a failure.
    pub(super) fn failures(&self) -> usize {
        self.entries.iter().filter(|e| e.findings.iter().any(|f| f.level == Level::Fail)).count()
    }

    /// The files with a warning and no failure.
    pub(super) fn warnings(&self) -> usize {
        self.entries.iter().filter(|e| e.findings.iter().all(|f| f.level == Level::Warn) && !e.findings.is_empty()).count()
    }

    /// One line for each file, the findings under it, and the count.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in &self.entries {
            let level = match entry.findings.iter().map(|f| f.level).max() {
                Some(Level::Fail) => "FAIL",
                Some(Level::Warn) => "warn",
                None => "ok",
            };
            let conditional = entry.conditional.map_or("-".to_owned(), |status| status.to_string());
            lines.push(format!(
                "{level:<4} {} | {} | {} | {} | 304? {conditional} | {} bytes | {}",
                entry.status,
                entry.cache_control.as_deref().unwrap_or("-"),
                entry.encoding.as_deref().unwrap_or("-"),
                entry.etag.as_deref().unwrap_or("-"),
                entry.size,
                entry.redirect.as_deref().map_or_else(|| entry.path.clone(), |to| format!("{} -> {to}", entry.path))
            ));
            for finding in &entry.findings {
                lines.push(format!("       {}: {}", if finding.level == Level::Fail { "FAIL" } else { "warn" }, finding.text));
            }
        }
        lines.push(format!("deploy-check: {} URLs, {} failures, {} warnings", self.entries.len(), self.failures(), self.warnings()));
        lines
    }
}

/// Whether a file of this kind is worth compressing, and so worth a warning when it comes back as it is.
fn compressible(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with('/')
        || [".wasm", ".js", ".mjs", ".css", ".html", ".json", ".webmanifest", ".txt", ".svg", ".xml"].iter().any(|end| lower.ends_with(end))
}

/// What is wrong with a response to a request for `path`; `conditional` is the status of the request that carried its `ETag`.
fn judge(path: &str, response: &Response, conditional: Option<u16>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut say = |level: Level, text: String| findings.push(Finding { level, text });
    if response.status != 200 {
        say(Level::Fail, format!("answered {}, and a file of the site is answered 200", response.status));
        return findings;
    }
    let value = response.header("cache-control");
    let shown = value.unwrap_or("no Cache-Control");
    let cache = value.map(cache::cache_control).unwrap_or_default();
    if cache::name_hash(path).is_some() {
        if !cache.lasts_a_year() {
            say(
                Level::Fail,
                format!(
                    "`{shown}`: a file named by its hash is never asked about again, so it must be `immutable` for a year (max-age={})",
                    cache::YEAR
                ),
            );
        }
    } else if cache.long_lived() {
        say(Level::Fail, format!("`{shown}`: this file is not named by a hash of its bytes, and a copy kept would hide the next release"));
    } else if value.is_none() {
        say(Level::Warn, "no Cache-Control: a browser would guess how long to keep it".to_owned());
    } else if cache.max_age != Some(0) && !cache.no_cache && !cache.no_store {
        say(Level::Warn, format!("`{shown}` does not say to ask again (max-age=0, must-revalidate, or no-cache)"));
    }
    if response.header("content-encoding").is_none() && compressible(path) && response.size >= COMPRESS_FROM {
        let what = if path.to_ascii_lowercase().ends_with(".wasm") { "a .wasm part" } else { "a text file" };
        say(Level::Warn, format!("{what} came back uncompressed ({} bytes) to a request for br and gzip", response.size));
    }
    if response.header("etag").is_none() {
        say(Level::Warn, "no ETag: a request cannot be answered 304".to_owned());
    } else if let Some(status) = conditional.filter(|status| *status != 304) {
        say(Level::Warn, format!("answered {status} to a request with its own ETag, and not 304"));
    }
    findings
}

/// The addresses without the extension that Pages sends the page at `path` to: `/x.html` to `/x`, `/index.html` to `/`, and `/dir/index.html` to `/dir/` (or `/dir`). Nothing for a path that is
/// not a page with an extension.
fn extensionless(path: &str) -> Vec<String> {
    let Some(stem) = path.strip_suffix(".html").filter(|stem| !stem.is_empty() && !stem.ends_with('/')) else { return Vec::new() };
    match stem.strip_suffix("/index") {
        Some("") => vec!["/".to_owned()],
        Some(folder) => vec![format!("{folder}/"), folder.to_owned()],
        None => vec![stem.to_owned()],
    }
}

/// The path of a `Location` that is the site's own: a path (not `//host`), or an address that starts with the site's own `origin`. A query, a fragment or anything that is not a plain address is
/// not taken.
fn own_path(fetch: &dyn Fetch, location: &str) -> Option<String> {
    if location.is_empty() || location.chars().any(|c| c.is_control() || c.is_whitespace()) || location.contains(['?', '#']) {
        return None;
    }
    if location.starts_with('/') {
        return (!location.starts_with("//")).then(|| location.to_owned());
    }
    let rest = location.strip_prefix(fetch.origin()?)?;
    if rest.is_empty() { Some("/".to_owned()) } else { rest.starts_with('/').then(|| rest.to_owned()) }
}

/// Where the 308 of `response` sends the page at `path`, if that is where Pages sends a page: its own address without the extension. The other case says what was wrong.
fn pages_redirect(fetch: &dyn Fetch, path: &str, response: &Response) -> Result<String, String> {
    let location = response.header("location");
    let wanted = extensionless(path);
    let went = location.and_then(|location| own_path(fetch, location));
    match went {
        Some(went) if wanted.contains(&went) => Ok(went),
        _ => Err(format!(
            "answered 308 {}, and a page with its extension is sent to its own address without it ({})",
            location.map_or("with no Location".to_owned(), |location| format!("to `{location}`")),
            if wanted.is_empty() {
                "this path is not a page".to_owned()
            } else {
                wanted.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(" or ")
            }
        )),
    }
}

/// The text of a response, as far as it is text (what `curl` gave is already at most [`MAX_BODY`] and a head).
fn text_of(response: &Response) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}

/// Looks at a site: finds its files from its start page, `/`, and asks for each.
pub(super) fn check(fetch: &dyn Fetch) -> Result<Report, String> {
    let home = fetch.get("/", None, true).map_err(|e| format!("the start page `/`: {e}"))?;
    if home.status != 200 {
        return Err(format!("the start page `/` answered {}: nothing can be found without it", home.status));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<(String, Option<String>)> = vec![("/".to_owned(), Some(text_of(&home)))];
    seen.insert("/".to_owned());
    while let Some((path, known)) = queue.pop() {
        let text = match known {
            Some(text) => Some(text),
            // A kind of file with references in it: its text, when it is there. A file that is not is reported when it is probed.
            None if has_references(&path) => read_text(fetch, &path),
            None => None,
        };
        for (reference, link) in text.map(|text| discover(&path, &text)).unwrap_or_default() {
            let Some(next) = resolve_ref(&path, &reference) else { continue };
            // A link is followed to a page of the site (its pictures are the site's too) and to nothing else: not to a download.
            if link && !is_page(&next) {
                continue;
            }
            if seen.insert(next.clone()) {
                if seen.len() > MAX_URLS {
                    return Err(format!(
                        "more than {MAX_URLS} files to look at: the check follows the files of a few pages and manifests, and no more"
                    ));
                }
                queue.push((next, None));
            }
        }
    }
    // Each file once. A page that Pages sends on is followed once, and its target is probed right after it, as a file of its own that is not sent on again.
    let mut entries = Vec::new();
    let mut probed: BTreeSet<String> = BTreeSet::new();
    for path in seen {
        if !probed.insert(path.clone()) {
            continue;
        }
        let (entry, target) = probe(fetch, &path, true);
        entries.push(entry);
        if let Some(target) = target
            && probed.insert(target.clone())
        {
            entries.push(probe(fetch, &target, false).0);
        }
    }
    // The app's site holds the registry's proof of the domain at a path nothing links.
    if text_of(&home).contains("id=\"diacraft_canvas\"") {
        entries.push(registry_proof(fetch));
    }
    Ok(Report { entries })
}

/// The registry's proof of the domain, asked for as the registry asks: a plain request, no redirect followed. It is probed as any file is (a status that is not 200 is a failure), and its type and body are
/// held to what the registry reads.
fn registry_proof(fetch: &dyn Fetch) -> Entry {
    let path = format!("/{}", app::REGISTRY_AUTH_PATH);
    let (mut entry, _) = probe(fetch, &path, false);
    if entry.status == 200 {
        match fetch.get(&path, None, true) {
            Ok(response) => {
                let kind = response.header("content-type").unwrap_or("");
                if !kind.to_ascii_lowercase().starts_with("text/plain") {
                    entry.findings.push(Finding {
                        level: Level::Fail,
                        text: format!(
                            "the content type is `{}`, and the registry asks for text/plain",
                            if kind.is_empty() { "not given" } else { kind }
                        ),
                    });
                }
                if let Err(why) = app::registry_record(&text_of(&response)) {
                    entry.findings.push(Finding {
                        level: Level::Fail,
                        text: format!(
                            "the body is not a record the registry reads (is it the app's page, which answers every path that has no file?): {why}"
                        ),
                    });
                }
            }
            Err(e) => entry.findings.push(Finding { level: Level::Fail, text: format!("the request for the body failed: {e}") }),
        }
    }
    entry
}

/// The text of a file that has references in it, when it is there: a page that Pages sends on (its address with the extension) is read at the address it is sent to.
fn read_text(fetch: &dyn Fetch, path: &str) -> Option<String> {
    let response = fetch.get(path, None, true).ok()?;
    match response.status {
        200 => Some(text_of(&response)),
        308 => {
            let target = pages_redirect(fetch, path, &response).ok()?;
            fetch.get(&target, None, true).ok().filter(|r| r.status == 200).map(|r| text_of(&r))
        }
        _ => None,
    }
}

/// One file: asked for as a browser asks, and again with its `ETag`. With `follow`, a page that Pages sends to its address without the extension is not a failure: the entry says where, and the
/// path it is sent to comes back, to be probed as a file of its own (without `follow`, so that a redirect of a redirect is an answer that is not 200).
fn probe(fetch: &dyn Fetch, path: &str, follow: bool) -> (Entry, Option<String>) {
    let mut entry = Entry {
        path: path.to_owned(),
        status: 0,
        cache_control: None,
        encoding: None,
        etag: None,
        size: 0,
        conditional: None,
        redirect: None,
        findings: Vec::new(),
    };
    let response = match fetch.get(path, None, false) {
        Ok(response) => response,
        Err(e) => {
            entry.findings.push(Finding { level: Level::Fail, text: format!("the request failed: {e}") });
            return (entry, None);
        }
    };
    entry.status = response.status;
    if follow && response.status == 308 && path.ends_with(".html") {
        entry.size = response.size;
        return match pages_redirect(fetch, path, &response) {
            Ok(target) => {
                entry.redirect = Some(target.clone());
                (entry, Some(target))
            }
            Err(why) => {
                entry.findings.push(Finding { level: Level::Fail, text: why });
                (entry, None)
            }
        };
    }
    entry.cache_control = response.header("cache-control").map(str::to_owned);
    entry.encoding = response.header("content-encoding").map(str::to_owned);
    entry.etag = response.header("etag").map(str::to_owned);
    entry.size = response.size;
    if response.status == 200
        && let Some(etag) = &entry.etag
    {
        match fetch.get(path, Some(etag), false) {
            Ok(second) => entry.conditional = Some(second.status),
            Err(e) => entry.findings.push(Finding { level: Level::Warn, text: format!("the request with the ETag failed: {e}") }),
        }
    }
    entry.findings.extend(judge(path, &response, entry.conditional));
    (entry, None)
}

/// `cargo xtask deploy-check <BASE_URL> [<BASE_URL>]`: each site given is checked on its own, in the order given, and the run fails if any of them has a failure.
pub(super) fn run(args: &[&str]) -> Result<(), String> {
    if args.is_empty() || args.len() > MAX_SITES {
        return Err(USAGE.into());
    }
    let sites = args.iter().map(|base| Curl::new(base)).collect::<Result<Vec<_>, _>>()?;
    if let [first, second] = sites.as_slice()
        && first.base == second.base
    {
        return Err(format!("both addresses are {}: the pages and the app are two sites, each at an address of its own\n{USAGE}", first.base));
    }
    let mut failures = 0;
    for (n, curl) in sites.iter().enumerate() {
        let which = if sites.len() > 1 { format!("site {} of {}: ", n + 1, sites.len()) } else { String::new() };
        println!("deploy-check: {which}{} (every request asks for br and gzip, and the second of each carries the ETag)", curl.base);
        let report = check(curl)?;
        for line in report.lines() {
            println!("{line}");
        }
        failures += report.failures();
    }
    match failures {
        0 => Ok(()),
        failures => Err(format!("deploy-check: {failures} files failed, above")),
    }
}
