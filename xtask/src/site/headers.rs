//! The two `_headers` files of the site: the pages' (`site/_headers`, for `https://diacraft.dev`) and the app's (`apps/diacraft-web/_headers`, which `cargo xtask site --web` puts in
//! the app's folder, for `https://diacraft.app`). Each is a file of its own, for a Cloudflare Pages project of its own, and neither names a path of the other: no rule is matched twice by a path, so nothing
//! has to be detached and a header is never sent twice. `cache.rs` reads each as Pages applies it, against the files it is deployed with; this holds what the file says: which policy, which headers, which
//! cache rules.

use std::collections::{BTreeMap, BTreeSet};

/// The policy of every page of the site, as (directive, its sources). A directive that is not here may be `'none'` or `'self'` and nothing else. The pages may fetch their own files, which
/// the check that Lighthouse's `robots.txt` audit makes needs (it asks as the page, under the page's policy: with no `'self'` in `connect-src` a valid robots file is reported invalid), and
/// GitHub's API, for the release lookup of the home page; they connect to nothing else.
const CSP: &[(&str, &[&str])] = &[
    ("default-src", &["'self'"]),
    ("script-src", &["'self'"]),
    ("connect-src", &["'self'", "https://api.github.com"]),
    ("img-src", &["'self'"]),
    ("style-src", &["'self'"]),
    ("frame-ancestors", &["'none'"]),
];

/// The policy of the app, and no looser than it has to be. WebAssembly needs `'wasm-unsafe-eval'` to compile, and nothing else of
/// `eval`. The page fetches its own wasm file, so connections are to the site itself and not to GitHub's API (the app makes no request of its own: the
/// browser run of counted them). Scripts and styles are files of the site: the build moves Trunk's inline start script out of the page
/// (`site/app.rs`), and the page's styles are a file (`apps/diacraft-web/page.css`).
const APP_CSP: &[(&str, &[&str])] = &[
    ("default-src", &["'self'"]),
    ("script-src", &["'self'", "'wasm-unsafe-eval'"]),
    ("connect-src", &["'self'"]),
    ("img-src", &["'self'"]),
    ("style-src", &["'self'"]),
    // The web app manifest is the app's own file: `default-src` would let the browser read it, and the directive says so.
    ("manifest-src", &["'self'"]),
    ("frame-ancestors", &["'none'"]),
    ("object-src", &["'none'"]),
    ("base-uri", &["'none'"]),
    ("form-action", &["'none'"]),
];

/// The route of the rules that every file of a site has.
const ALL: &str = "/*";

/// The routes of the app's hashed files: the parts of its module and the data it fetches, and the three files Trunk makes at the app's root, which `cargo xtask site --web`
/// renames by the hash of their bytes: the glue script, the page's style and its icon. Their names carry their hash, so a file never changes under its name and is
/// cached for good; each rule sets a cache header and nothing else. `cache.rs` holds the other half: that no file whose name is not a hash of its own bytes is matched by a rule that caches it.
/// They are paths of the app's own site (the app is at its root), where they were under `/app/` while the app was a folder of the pages.
pub(super) const APP_CACHE_ROUTES: [&str; 5] = ["/parts/*", "/assets/*", "/diacraft-web-*.js", "/page-*.css", "/diacraft-icon-256-*.png"];

/// What every file of either site is sent with besides what a rule of its own says.
const SECURITY: &[&str] = &["content-security-policy", "x-content-type-options", "referrer-policy", "permissions-policy"];

/// The policy of an ordinary page of the site.
pub(super) fn csp(value: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for directive in value.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut words = directive.split_ascii_whitespace();
        let name = words.next().unwrap_or("");
        if !seen.insert(name) {
            return Err(format!("duplicate CSP directive {name}"));
        }
        if let Some((_, expected)) = CSP.iter().find(|(directive, _)| *directive == name) {
            let sources: BTreeSet<&str> = words.collect();
            if sources != expected.iter().copied().collect::<BTreeSet<_>>() {
                return Err(format!("expected {name} {}", expected.join(" ")));
            }
        } else if !matches!(words.collect::<Vec<_>>().as_slice(), ["'none'"] | ["'self'"] | []) {
            return Err(format!("additional CSP directive permits other sources: {directive}"));
        }
    }
    for (name, _) in CSP {
        if !seen.contains(name) {
            return Err(format!("missing CSP directive {name}"));
        }
    }
    Ok(())
}

/// The policy of the app: exactly [`APP_CSP`], in any order, with no other directive and no other source.
pub(super) fn app_csp(value: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for directive in value.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut words = directive.split_ascii_whitespace();
        let name = words.next().unwrap_or("");
        if !seen.insert(name) {
            return Err(format!("duplicate CSP directive {name} in the app's policy"));
        }
        let Some((_, expected)) = APP_CSP.iter().find(|(directive, _)| *directive == name) else {
            return Err(format!("the app's policy has the directive {name}, which it does not need"));
        };
        let sources: BTreeSet<&str> = words.collect();
        if sources != expected.iter().copied().collect::<BTreeSet<_>>() {
            return Err(format!(
                "the app's {name} is `{}`, and must be `{}` and nothing more",
                sources.into_iter().collect::<Vec<_>>().join(" "),
                expected.join(" ")
            ));
        }
    }
    for (name, _) in APP_CSP {
        if !seen.contains(name) {
            return Err(format!("the app's policy is missing {name}"));
        }
    }
    Ok(())
}

/// The value of a security header: the policy by `policy` (the pages' or the app's), and the others as the two sites hold them alike.
fn security(name: &str, value: &str, policy: fn(&str) -> Result<(), String>) -> Result<(), String> {
    let valid = match name {
        "content-security-policy" => return policy(value),
        "x-content-type-options" => value.eq_ignore_ascii_case("nosniff"),
        "referrer-policy" => matches!(value, "no-referrer" | "same-origin" | "strict-origin" | "strict-origin-when-cross-origin"),
        "permissions-policy" => value
            .split(',')
            .all(|item| item.trim().split_once('=').is_some_and(|(feature, policy)| !feature.trim().is_empty() && policy.trim() == "()")),
        _ => true,
    };
    if valid { Ok(()) } else { Err(format!("invalid {name}: {value}")) }
}

/// One header line of the file: (the route it is under, its lower-case name, its value), in order. The comment and blank lines are gone. A line that detaches a header (`! Name`) is an error:
/// each site has one file, whose rules set no header twice, so nothing is to be taken away (the app, which was a block of the pages' file, had to detach the pages' policy).
fn lines(text: &str) -> Result<Vec<(String, String, String)>, String> {
    let mut route = "";
    let mut found = Vec::new();
    for line in text.lines().filter(|s| !s.trim().is_empty() && !s.trim_start().starts_with('#')) {
        if !line.starts_with([' ', '\t']) {
            route = line.trim();
            continue;
        }
        let line = line.trim();
        if line.starts_with('!') {
            return Err(format!(
                "`{line}` under {route}: no header is detached: this site's file has no two rules that set one header, so there is nothing to take away"
            ));
        }
        let (name, value) = line.split_once(':').ok_or_else(|| format!("expected a header under {route}: {line}"))?;
        found.push((route.to_owned(), name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok(found)
}

/// Whether a `Cache-Control` value keeps a file for good: `immutable`, and a `max-age` of at least 30 days.
fn keeps_for_good(value: &str) -> bool {
    let directives: Vec<_> = value.split(',').map(str::trim).collect();
    let age = directives.iter().find_map(|d| d.strip_prefix("max-age=")?.parse::<u64>().ok());
    directives.contains(&"immutable") && age.unwrap_or(0) >= 2_592_000
}

/// What the pages' `_headers` file holds that the checks care about.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Found {
    /// The policy of the pages (`/*`) says `font-src 'self'`: a site that has fonts must say where they may come from. [`csp`] has already refused any
    /// other source for it.
    pub(super) font_src: bool,
}

/// Whether a policy has the directive `font-src 'self'`, with that one source.
fn names_font_src(policy: &str) -> bool {
    policy.split(';').any(|directive| directive.split_ascii_whitespace().eq(["font-src", "'self'"]))
}

/// The pages' file (`site/_headers`): the policy and the three headers under `/*`, each once, and cache rules under other routes (`hashed`: the folder of the pictures named by their hash is
/// there, and its rule keeps them). The pages hold nothing of the app: a rule for a path of the app, or a policy that is not the pages', is refused.
pub(super) fn check_pages(text: &str, hashed: bool) -> Result<Found, String> {
    let mut global = BTreeSet::new();
    let mut cache = BTreeMap::new();
    let mut font_src = false;
    for (route, name, value) in lines(text)? {
        if route == "/app" || route.starts_with("/app/") {
            return Err(format!(
                "a rule for {route}: the web app is a site of its own (apps/diacraft-web/_headers), and the pages' file has no rule for it"
            ));
        }
        if name == "content-security-policy" && route != ALL {
            return Err(format!("a policy under {route}: only {ALL} sets one"));
        }
        security(&name, &value, csp)?;
        if route == ALL && !global.insert(name.clone()) {
            return Err(format!("duplicate {name} under {ALL}"));
        }
        if route == ALL && name == "content-security-policy" {
            font_src = names_font_src(&value);
        }
        if name == "cache-control" {
            cache.insert(route, value);
        }
    }
    for name in SECURITY {
        if !global.contains(*name) {
            return Err(format!("{ALL} is missing {name}"));
        }
    }
    if hashed {
        let value = cache.get("/assets/hashed/*").ok_or("missing /assets/hashed/* cache-control")?;
        if !keeps_for_good(value) {
            return Err("hashed assets need immutable caching for at least 30 days".into());
        }
    }
    Ok(Found { font_src })
}

/// The app's file (`apps/diacraft-web/_headers`, put in the app's folder by `cargo xtask site --web`): under `/*` the app's own policy and the three headers every page has, each once and no other
/// (the app needs no cross-origin isolation: it uses no threads), and one rule for each of [`APP_CACHE_ROUTES`], setting a cache header and nothing else, that keeps the file for good. No other route:
/// the policy and the caching of the app are frozen by this, and a rule for another path is a change of this check.
pub(super) fn check_app(text: &str) -> Result<(), String> {
    let mut global = BTreeSet::new();
    let mut cache = BTreeMap::new();
    for (route, name, value) in lines(text)? {
        if route == ALL {
            if !SECURITY.contains(&name.as_str()) {
                return Err(format!(
                    "{ALL} sets {name}: the app's headers are its policy and the three that every page of the site has, and nothing else (the app needs no cross-origin isolation)"
                ));
            }
            security(&name, &value, app_csp)?;
            if !global.insert(name.clone()) {
                return Err(format!("duplicate {name} under {ALL}"));
            }
        } else if APP_CACHE_ROUTES.contains(&route.as_str()) {
            if name != "cache-control" {
                return Err(format!("{route} sets {name}: the rule for the app's hashed files sets a cache header and nothing else"));
            }
            if !keeps_for_good(&value) {
                return Err(format!("{route}: the app's hashed files need immutable caching for at least 30 days"));
            }
            cache.insert(route, value);
        } else {
            return Err(format!(
                "a rule for {route}: the app's headers are {ALL} and the rules for its hashed files ({}), and no other",
                APP_CACHE_ROUTES.join(", ")
            ));
        }
    }
    for name in SECURITY {
        if !global.contains(*name) {
            return Err(format!("{ALL} is missing {name}: the app runs under its own policy, which this file must set"));
        }
    }
    if let Some(route) = APP_CACHE_ROUTES.iter().find(|route| !cache.contains_key(**route)) {
        return Err(format!(
            "there is no rule for {route}: the hashed files of the app (each of {}) must be cached for good",
            APP_CACHE_ROUTES.join(", ")
        ));
    }
    Ok(())
}
