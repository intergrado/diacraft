//! What a browser is told to keep: `_headers` read the way Cloudflare Pages applies it, and the rules that hold the result.
//!
//! **The reading.** Pages applies every rule whose pattern matches the request path, in the order of the file; a header that two rules set is sent once, with the values joined by
//! a comma; `! Name` takes away what the rules before it added. A pattern is a path with one splat (`*`, which matches any characters, `/` too) and any number of placeholders
//! (`:name`, which match one or more characters but not `/`, and may be in the middle of a segment). [`Headers::resolve`] does that for one path. What this does not do: a rule for a
//! host (`https://...`) is refused, and so is a value that refers to a placeholder or the splat (`:name`, `:splat`), since nothing here substitutes them and nothing in the site's
//! file uses them.
//!
//! **The rules** ([`check`]). A file is kept for good only if its name says what it holds: `<stem>-<16 hex digits>.<ext>` or `<stem>.<12 hex digits>.<ext>`, where the digits begin the
//! SHA-256 of the file's own bytes. For every file the check asks what Pages would send at every address the file is served at:
//! - a file whose name is not such a hash (or whose hash is not its own) is not given `immutable` and no `max-age` or `s-maxage` above 0, or a release could stay in browsers for
//!   a year (`index.html`, `start.js`, the manifests, the snippets of the glue: their folder is a hash of a crate's name and version, and not of the file);
//! - a file whose name is its own hash is given `immutable` and a year, or a visitor asks about it again on every visit;
//! - two `Cache-Control` values for one address would be sent as one joined value, which a browser reads by its own rule: refused;
//! - every page is sent with a content security policy, one policy and not two, as strict as the site holds it (the pages' own, or the app's: [`Kind`]).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{headers, parts};

/// A year in seconds: what a file kept for good is given as its `max-age`.
pub(super) const YEAR: u64 = 31_536_000;

/// What Pages accepts in `_headers` (its documentation, "Headers"): up to 100 rules, and 2,000 characters in a line.
const MAX_RULES: usize = 100;
const MAX_LINE: usize = 2000;

/// The most pieces a pattern has, and the longest path it is matched against: the matcher's work is the pieces times the square of the path.
const MAX_TOKENS: usize = 64;
const MAX_PATH: usize = 1024;

/// The most files the check looks at, and the most it reads of one (it reads a file to hash it, and only when its name looks like a hash).
const MAX_FILES: usize = 4096;
pub(super) const MAX_READ: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(Vec<char>),
    /// `:name`: one or more characters but `/`.
    Placeholder,
    /// `*`: any characters.
    Splat,
}

/// A path pattern of `_headers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pattern {
    tokens: Vec<Token>,
    /// The names a value may refer to with a colon: the placeholders', and `splat` when there is a splat.
    names: Vec<String>,
}

/// The pieces of `part` (a pattern with no splat in it): text and placeholders. `names` collects the placeholders' names.
fn scan(part: &str, names: &mut Vec<String>, tokens: &mut Vec<Token>) -> Result<(), String> {
    let chars: Vec<char> = part.chars().collect();
    let mut literal: Vec<char> = Vec::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let starts_name = c == ':' && chars.get(at + 1).is_some_and(char::is_ascii_alphabetic);
        if !starts_name {
            literal.push(c);
            at += 1;
            continue;
        }
        if !literal.is_empty() {
            tokens.push(Token::Literal(std::mem::take(&mut literal)));
        }
        let name: String = chars.iter().skip(at + 1).take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
        at += 1 + name.chars().count();
        if name == "splat" {
            return Err("`:splat` is the name of the splat, not of a placeholder".into());
        }
        if names.contains(&name) {
            return Err(format!("the placeholder `:{name}` is used twice, and Pages takes each name once"));
        }
        names.push(name);
        tokens.push(Token::Placeholder);
    }
    if !literal.is_empty() {
        tokens.push(Token::Literal(literal));
    }
    Ok(())
}

impl Pattern {
    pub(super) fn compile(text: &str) -> Result<Pattern, String> {
        if text.starts_with("https://") || text.starts_with("http://") {
            return Err(format!("`{text}` is a rule for a host: this check reads rules for paths only"));
        }
        if !text.starts_with('/') {
            return Err(format!("`{text}` must start with /"));
        }
        let mut names = Vec::new();
        let mut tokens = Vec::new();
        match text.split_once('*') {
            Some((_, after)) if after.contains('*') => return Err(format!("`{text}` has more than one splat, and Pages accepts one")),
            Some((before, after)) => {
                scan(before, &mut names, &mut tokens)?;
                tokens.push(Token::Splat);
                names.push("splat".to_owned());
                scan(after, &mut names, &mut tokens)?;
            }
            None => scan(text, &mut names, &mut tokens)?,
        }
        if tokens.len() > MAX_TOKENS {
            return Err(format!("`{text}` has more than {MAX_TOKENS} pieces"));
        }
        Ok(Pattern { tokens, names })
    }

    /// Whether the whole of `path` is matched. Reachable positions are carried from piece to piece, so the work is bounded by the pieces and the square of the path.
    pub(super) fn matches(&self, path: &str) -> bool {
        let chars: Vec<char> = path.chars().collect();
        if chars.len() > MAX_PATH {
            return false;
        }
        let end = chars.len();
        let mut reach = vec![false; end + 1];
        if let Some(first) = reach.first_mut() {
            *first = true;
        }
        for token in &self.tokens {
            let mut next = vec![false; end + 1];
            for from in (0..=end).filter(|p| reach.get(*p).copied().unwrap_or(false)) {
                match token {
                    Token::Literal(text) => {
                        let to = from + text.len();
                        if chars.get(from..to) == Some(text.as_slice())
                            && let Some(slot) = next.get_mut(to)
                        {
                            *slot = true;
                        }
                    }
                    Token::Placeholder => {
                        for to in from + 1..=end {
                            if chars.get(to - 1) == Some(&'/') {
                                break;
                            }
                            if let Some(slot) = next.get_mut(to) {
                                *slot = true;
                            }
                        }
                    }
                    Token::Splat => {
                        for slot in next.iter_mut().skip(from) {
                            *slot = true;
                        }
                    }
                }
            }
            reach = next;
        }
        reach.get(end).copied().unwrap_or(false)
    }
}

/// One line under a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Set { name: String, value: String },
    Detach(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    pattern: Pattern,
    ops: Vec<Op>,
}

/// What a path is sent with: header name (lower case) to its values, in the order the rules added them.
pub(super) type Resolved = BTreeMap<String, Vec<String>>;

/// The rules of a `_headers` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Headers {
    rules: Vec<Rule>,
}

/// A header name: the characters of an HTTP token.
fn header_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

impl Headers {
    pub(super) fn parse(text: &str) -> Result<Headers, String> {
        let mut rules: Vec<Rule> = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let at = index + 1;
            if line.chars().count() > MAX_LINE {
                return Err(format!("line {at}: more than {MAX_LINE} characters, which Pages does not read"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !line.starts_with([' ', '\t']) {
                if rules.len() >= MAX_RULES {
                    return Err(format!("line {at}: more than {MAX_RULES} rules, which Pages does not read"));
                }
                let pattern = Pattern::compile(trimmed).map_err(|e| format!("line {at}: {e}"))?;
                rules.push(Rule { pattern, ops: Vec::new() });
                continue;
            }
            let Some(rule) = rules.last_mut() else {
                return Err(format!("line {at}: a header before any URL: `{trimmed}`"));
            };
            if let Some(name) = trimmed.strip_prefix('!') {
                let name = name.trim();
                if name.contains(':') {
                    return Err(format!("line {at}: a detach names the header only (`! Name`): `{trimmed}`"));
                }
                if !header_name(name) {
                    return Err(format!("line {at}: `{name}` is not a header name"));
                }
                rule.ops.push(Op::Detach(name.to_ascii_lowercase()));
                continue;
            }
            let (name, value) = trimmed.split_once(':').ok_or_else(|| format!("line {at}: expected a header (`Name: value`): `{trimmed}`"))?;
            let (name, value) = (name.trim(), value.trim());
            if !header_name(name) {
                return Err(format!("line {at}: `{name}` is not a header name"));
            }
            if value.is_empty() {
                return Err(format!("line {at}: the header {name} has no value"));
            }
            if let Some(referred) = rule.pattern.names.iter().find(|n| value.contains(&format!(":{n}"))) {
                return Err(format!("line {at}: the value refers to :{referred}, which this check does not substitute"));
            }
            rule.ops.push(Op::Set { name: name.to_ascii_lowercase(), value: value.to_owned() });
        }
        Ok(Headers { rules })
    }

    /// What Pages sends with `path`: every rule that matches, in order, each of its lines applied in order.
    pub(super) fn resolve(&self, path: &str) -> Resolved {
        let mut sent = Resolved::new();
        for rule in self.rules.iter().filter(|rule| rule.pattern.matches(path)) {
            for op in &rule.ops {
                match op {
                    Op::Detach(name) => {
                        sent.remove(name);
                    }
                    Op::Set { name, value } => sent.entry(name.clone()).or_default().push(value.clone()),
                }
            }
        }
        sent
    }
}

/// The directives of a `Cache-Control` value that matter here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Cache {
    pub(super) max_age: Option<u64>,
    pub(super) s_maxage: Option<u64>,
    pub(super) immutable: bool,
    pub(super) no_store: bool,
    pub(super) no_cache: bool,
}

/// A number of seconds: digits only. A number too large for 64 bits is a very long life, and not none.
fn seconds(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.parse::<u64>().unwrap_or(u64::MAX))
}

pub(super) fn cache_control(value: &str) -> Cache {
    let mut cache = Cache::default();
    for directive in value.split(',') {
        let (name, argument) = match directive.split_once('=') {
            Some((name, argument)) => (name.trim(), Some(argument.trim().trim_matches('"'))),
            None => (directive.trim(), None),
        };
        match name.to_ascii_lowercase().as_str() {
            "max-age" => cache.max_age = argument.and_then(seconds),
            "s-maxage" => cache.s_maxage = argument.and_then(seconds),
            "immutable" => cache.immutable = true,
            "no-store" => cache.no_store = true,
            "no-cache" => cache.no_cache = true,
            _ => {}
        }
    }
    cache
}

impl Cache {
    /// `immutable` and a `max-age` of a year or more, and nothing that says not to keep it.
    pub(super) fn lasts_a_year(&self) -> bool {
        self.immutable && self.max_age.is_some_and(|age| age >= YEAR) && !self.no_store && !self.no_cache
    }

    /// Anything that lets a browser or a shared cache use its copy without asking.
    pub(super) fn long_lived(&self) -> bool {
        self.immutable || self.max_age.is_some_and(|age| age > 0) || self.s_maxage.is_some_and(|age| age > 0)
    }
}

fn is_lower_hex(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `<stem>-<16 digits>`: the digits of the end of `base`, when something comes before the dash.
fn dashed(base: &str) -> Option<&str> {
    let split = base.len().checked_sub(17)?;
    let (stem, rest) = (base.get(..split)?, base.get(split..)?);
    let digits = rest.strip_prefix('-')?;
    (!stem.is_empty() && is_lower_hex(digits)).then_some(digits)
}

/// `<stem>.<12 digits>`.
fn dotted(base: &str) -> Option<&str> {
    let (stem, digits) = base.rsplit_once('.')?;
    (!stem.is_empty() && digits.len() == 12 && is_lower_hex(digits)).then_some(digits)
}

/// The hexadecimal digits a file name carries as its hash, if its shape is one of a hashed file's: `<stem>-<16 digits>.<ext>` (the parts of the module, the data beside them, the files
/// Trunk made) or `<stem>.<12 digits>.<ext>` (the site's pictures). Only the name of the file counts, and not the folders: a folder named by a hash of something else is no hash of this file.
pub(super) fn name_hash(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next()?;
    let (base, extension) = file.rsplit_once('.')?;
    if extension.is_empty() {
        return None;
    }
    dashed(base).or_else(|| dotted(base))
}

/// The request paths a file is served at: a page by its name with and without `.html`, a folder's `index.html` by the folder and by its name.
pub(super) fn request_paths(name: &str) -> Vec<String> {
    if name == "index.html" {
        return vec!["/".to_owned(), "/index.html".to_owned()];
    }
    if let Some(folder) = name.strip_suffix("/index.html") {
        return vec![format!("/{folder}/"), format!("/{name}")];
    }
    if let Some(stem) = name.strip_suffix(".html") {
        return vec![format!("/{stem}"), format!("/{name}")];
    }
    vec![format!("/{name}")]
}

/// Which of the two sites a folder is: the pages, or the web app. They are deployed to two Pages projects, each with its own `_headers`, and a page of each is held to its own policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// The pages of `site/`, for `https://diacraft.dev`: the strict policy of [`headers::csp`].
    Pages,
    /// The web app, for `https://diacraft.app`, with its start page at the root: the policy of [`headers::app_csp`].
    App,
}

/// What the check found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Summary {
    /// Files named by their own hash, each kept for a year.
    pub(super) kept: usize,
    /// The other files, each asked about on every visit.
    pub(super) revalidated: usize,
    /// HTML pages, each with its policy.
    pub(super) pages: usize,
}

impl Summary {
    /// The line `cargo xtask site` prints.
    pub(super) fn line(&self) -> String {
        format!(
            "site: caching: {} files named by their own hash are kept for a year, {} are asked about on every visit, and {} pages have one content security policy",
            self.kept, self.revalidated, self.pages
        )
    }
}

/// How a file's name stands to its bytes.
enum Named<'a> {
    /// The name carries the beginning of the SHA-256 of the file's bytes.
    Own,
    /// The name carries digits that are not the file's: `carried`, and the digits the bytes have, as many.
    Foreign { carried: &'a str, actual: String },
    /// The name carries no hash.
    Plain,
}

/// The one policy a page is sent with, and as strict as the site holds it (the pages' own, or the app's).
fn policy(name: &str, path: &str, resolved: &Resolved, kind: Kind) -> Result<(), String> {
    let values = resolved.get("content-security-policy").map(Vec::as_slice).unwrap_or(&[]);
    match values {
        [] => Err(format!("{name}: at {path} Pages would send no content security policy")),
        [one] => {
            let held = match kind {
                Kind::Pages => headers::csp(one),
                Kind::App => headers::app_csp(one),
            };
            held.map_err(|e| format!("{name}: at {path} the policy is not the one the site holds: {e}"))
        }
        many => Err(format!(
            "{name}: at {path} Pages would send {} content security policies, joined with a comma, and a browser would apply them all: `! Content-Security-Policy` before the one that replaces the page's",
            many.len()
        )),
    }
}

/// A file of `dir`, read whole: a regular file (no link) of at most [`MAX_READ`] bytes.
fn read_in(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = dir.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|e| format!("{name}: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_READ {
        return Err(format!("{name}: expected a regular file of at most {MAX_READ} bytes"));
    }
    fs::read(&path).map_err(|e| format!("{name}: {e}"))
}

/// [`check`] for the folder `dir` that holds `_headers` and `names` (its files, from `inventory`), a site of this `kind`.
pub(super) fn check_dir(dir: &Path, names: &[String], kind: Kind) -> Result<Summary, String> {
    let text = String::from_utf8(read_in(dir, "_headers")?).map_err(|_| "_headers: expected UTF-8".to_owned())?;
    check(&text, names, &|name| read_in(dir, name), kind)
}

/// Checks the `_headers` text against the files of the folder it is deployed with (`files`: their paths from the folder, `/` separated; `read` gives the bytes of one, and is asked
/// for a file whose name looks like a hash only), a site of this `kind`. Fails at the first fault, naming the file and the address.
pub(super) fn check(headers_text: &str, files: &[String], read: &dyn Fn(&str) -> Result<Vec<u8>, String>, kind: Kind) -> Result<Summary, String> {
    let rules = Headers::parse(headers_text).map_err(|e| format!("_headers: {e}"))?;
    if files.len() > MAX_FILES {
        return Err(format!("more than {MAX_FILES} files: the check reads no more than that"));
    }
    let mut names: Vec<&str> = files.iter().map(String::as_str).filter(|name| *name != "_headers").collect();
    names.sort_unstable();
    let mut summary = Summary::default();
    for name in names {
        let named = match name_hash(name) {
            None => Named::Plain,
            Some(carried) => {
                let digest = parts::sha256(&read(name)?);
                if digest.starts_with(carried) {
                    Named::Own
                } else {
                    Named::Foreign { carried, actual: digest.get(..carried.len()).unwrap_or(&digest).to_owned() }
                }
            }
        };
        for path in request_paths(name) {
            let resolved = rules.resolve(&path);
            let values = resolved.get("cache-control").map(Vec::as_slice).unwrap_or(&[]);
            if values.len() > 1 {
                return Err(format!(
                    "{name}: at {path} Pages would send {} Cache-Control values joined as one (`{}`): a header that two rules set is joined with a comma, and what a browser makes of that is its own; `! Cache-Control` first takes the earlier away",
                    values.len(),
                    values.join(", ")
                ));
            }
            let shown = values.first().map_or("no Cache-Control", String::as_str);
            let cache = values.first().map(|value| cache_control(value)).unwrap_or_default();
            match &named {
                Named::Own if !cache.lasts_a_year() => {
                    return Err(format!(
                        "{name}: is named by the hash of its own bytes, and at {path} `_headers` gives it `{shown}`: it must be `immutable` for a year (max-age={YEAR}), or a visitor asks about it again on every visit"
                    ));
                }
                Named::Foreign { carried, actual } if cache.long_lived() => {
                    return Err(format!(
                        "{name}: is not named by a hash of its own bytes (its name carries `{carried}` and its bytes hash to `{actual}`), and at {path} `_headers` gives it `{shown}`: a file that changes under its name must be asked about on every visit, or a release could stay in browsers for a year"
                    ));
                }
                Named::Plain if cache.long_lived() => {
                    return Err(format!(
                        "{name}: is not named by a hash of its own bytes, and at {path} `_headers` gives it `{shown}`: a file that changes under its name must be asked about on every visit, or a release could stay in browsers for a year"
                    ));
                }
                _ => {}
            }
            if name.ends_with(".html") {
                policy(name, &path, &resolved, kind)?;
            }
        }
        if matches!(named, Named::Own) {
            summary.kept += 1;
        } else {
            summary.revalidated += 1;
        }
        if name.ends_with(".html") {
            summary.pages += 1;
        }
    }
    Ok(summary)
}
