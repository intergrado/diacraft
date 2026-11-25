//! Offline checks for the two folders served by Pages: the pages (`site/`, for `https://diacraft.dev`) and, with `--web`, the web app at the root of a site of its own (for
//! `https://diacraft.app`).

mod app;
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use app::{leb128, size_parts, wasm_shared_memory};
#[cfg(test)]
mod app_tests;
mod cache;
#[cfg(test)]
mod cache_tests;
mod downloads;
mod headers;
mod html;
mod links;
mod live;
#[cfg(test)]
mod live_tests;
mod parts;
#[cfg(test)]
mod parts_tests;
mod seo;
#[cfg(test)]
mod seo_tests;
mod settings;
#[cfg(test)]
mod tests;
mod web_manifest;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use links::Origins;

const PAGE_BUDGET: u64 = 1_000_000;

#[derive(Default)]
struct File {
    bytes: u64,
    ids: BTreeSet<String>,
    links: Vec<String>,
    assets: Vec<String>,
    /// For `_headers`: the policy of the pages says `font-src 'self'`.
    font_src: bool,
}

/// The kinds of font file. A font of the site is a `.woff2` in `assets/hashed/` and no other: named there by the hash of its bytes, it is kept for a year as
/// the pictures are (`cache.rs` holds the name to the bytes), and one format is all a current browser asks for.
const FONT_KINDS: [&str; 5] = ["woff2", "woff", "ttf", "otf", "eot"];
/// The folder of the files named by their hash.
const HASHED_DIR: &str = "assets/hashed/";

/// Whether `name` is a font file, by its extension in any case.
fn is_font(name: &str) -> bool {
    Path::new(name).extension().and_then(|ext| ext.to_str()).is_some_and(|ext| FONT_KINDS.iter().any(|kind| ext.eq_ignore_ascii_case(kind)))
}

const USAGE: &str = "usage: cargo xtask site [--write|--release|--web [--dist DIR] [--out DIR] [--app-out DIR] [--app-url URL]]";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Check,
    Write,
    Release,
    /// Build the web app, put it in the site and check the result.
    Web(app::Web),
}

fn parse(args: &[&str]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Check),
        ["--write"] => Ok(Mode::Write),
        ["--release"] => Ok(Mode::Release),
        ["--web", rest @ ..] => {
            let mut web = app::Web::default();
            let mut rest = rest.iter();
            while let Some(flag) = rest.next() {
                let value = match rest.next() {
                    Some(value) if !value.is_empty() && !value.starts_with("--") => *value,
                    _ => return Err(USAGE.into()),
                };
                match *flag {
                    "--dist" if web.dist.is_none() => web.dist = Some(value.into()),
                    "--out" if web.out.is_none() => web.out = Some(value.into()),
                    "--app-out" if web.app_out.is_none() => web.app_out = Some(value.into()),
                    "--app-url" if web.app_url.is_none() => web.app_url = Some(value.to_owned()),
                    _ => return Err(USAGE.into()),
                }
            }
            Ok(Mode::Web(web))
        }
        _ => Err(USAGE.into()),
    }
}

/// `cargo xtask deploy-check <BASE_URL>`: what a deployed site sends, against what `_headers` says (`live.rs`).
pub(super) fn deploy_check(args: &[&str]) -> Result<(), String> {
    live::run(args)
}

/// The version in `[workspace.package]` of the root `Cargo.toml` of the repository at `root`: the release the pages are made for.
fn workspace_version(root: &Path) -> Result<String, String> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    xtask::version::read(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The files of the repository that hold the licences of the code, from `license` in `[workspace.package]` of the root `Cargo.toml` of the repository at `root` (`seo::LICENSE_FILES`): each is a file of
/// `root`, and a licence with no such file is an error, so that the pages never name a file that is not there.
fn workspace_licences(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: toml::Table = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    let expression = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("license"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{}: [workspace.package] has no license", path.display()))?;
    let files = seo::licence_files(expression).map_err(|e| format!("{}: {e}", path.display()))?;
    for file in &files {
        if !root.join(file).is_file() {
            return Err(format!("{}: the licence file {file} is not in the repository's root", root.display()));
        }
    }
    Ok(files.into_iter().map(str::to_owned).collect())
}

/// What a build for a preview names as the app's address when `--app-url` is not given: `DIACRAFT_APP_URL`.
const APP_URL_ENV: &str = "DIACRAFT_APP_URL";

pub(super) fn run(dir: &Path, args: &[&str]) -> Result<(), String> {
    run_with(dir, args, std::env::var(APP_URL_ENV).ok().as_deref())
}

/// [`run`], with the value of `DIACRAFT_APP_URL` given (the tests give their own).
fn run_with(dir: &Path, args: &[&str], env_app_url: Option<&str>) -> Result<(), String> {
    let mode = parse(args)?;
    // The pages hold no app: it is a site of its own, made by `--web` into a folder of its own.
    if fs::symlink_metadata(dir.join(app::APP_DIR)).is_ok() {
        return Err(format!(
            "{}: the pages hold no app: it is a site of its own, made by `cargo xtask site --web` into a folder of its own",
            dir.join(app::APP_DIR).display()
        ));
    }
    // Inspect the deployment tree before following or writing any generated path.
    let files = inventory(dir)?;
    // The pages are made for the release of the workspace's version: each download link is a file of that release.
    let root = dir.parent().ok_or_else(|| format!("{}: the site folder has no parent", dir.display()))?;
    let mut settings = settings::load(dir, &workspace_version(root)?)?;
    settings.licences = workspace_licences(root)?;
    if mode == Mode::Release {
        settings::release(&settings)?;
    }
    let write = mode == Mode::Write;
    let files = if write {
        for name in files.iter().filter(|n| n.ends_with(".html")) {
            let path = dir.join(name);
            if path.metadata().map_err(|e| format!("{name}: {e}"))?.len() > PAGE_BUDGET {
                return Err(format!("{name}: HTML alone exceeds the {PAGE_BUDGET}-byte budget"));
            }
            let bytes = fs::read(path).map_err(|e| format!("{name}: {e}"))?;
            html::utf8(&bytes).and_then(html::before_write).map_err(|e| format!("{name}: {e}"))?;
        }
        // The dates of the sitemap are the last commits' own: `--write` is the one command that reads the history.
        settings::apply_with(dir, &settings, true, &settings::DateSource::Git(root))?;
        // Generation may have created robots.txt and sitemap.xml.
        inventory(dir)?
    } else {
        files
    };
    let (count, caching) = inspect(dir, &Origins::new(&settings.domain, &settings.app_url), files)?;
    if !write {
        settings::apply(dir, &settings, false)?;
    }
    // Every download link is a file that the release workflow uploads and lists, and `site.json` names no other.
    let pages: Vec<String> = inventory(dir)?.into_iter().filter(|name| name.ends_with(".html")).collect();
    downloads::check(root, dir, &settings, &pages)?;
    println!("site: {count} pages checked, each within {PAGE_BUDGET} bytes");
    println!("{}", caching.line());
    if let Mode::Web(web) = &mode {
        // A preview names the app on this machine: the option first, then the environment variable.
        let preview = web.app_url.as_deref().or(env_app_url.filter(|value| !value.is_empty()));
        let preview = preview.map(settings::app_override).transpose()?;
        app::run(dir, &settings, preview, web)?;
    }
    Ok(())
}

/// The check of a folder of the pages that `--web` made (`dir`, a copy of `site/`), for the settings it was made with: the same check as of `site/`, and the generated values current.
fn check_pages(dir: &Path, settings: &settings::Settings) -> Result<(usize, cache::Summary), String> {
    let found = inspect(dir, &Origins::new(&settings.domain, &settings.app_url), inventory(dir)?)?;
    settings::apply(dir, settings, false)?;
    Ok(found)
}

fn inventory(dir: &Path) -> Result<Vec<String>, String> {
    if !fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?.is_dir() {
        return Err(format!("{}: expected a directory", dir.display()));
    }
    let mut dirs = vec![dir.to_owned()];
    let mut files = Vec::new();
    while let Some(folder) = dirs.pop() {
        for entry in folder.read_dir().map_err(|e| format!("{}: {e}", folder.display()))? {
            let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() {
                let relative = path.strip_prefix(dir).map_err(|e| e.to_string())?;
                files.push(relative.to_str().ok_or("site paths must be UTF-8")?.replace(std::path::MAIN_SEPARATOR, "/"));
            } else {
                return Err(format!("{}: only regular files may be deployed", path.display()));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The texts the repository's brands check allows in the site's file `name` (`site/<name>` in the repository): the format lists and the
/// trademark line the owner allows (`xtask::brands::PRODUCT_USES`).
fn allowed(name: &str) -> Vec<&'static str> {
    xtask::brands::allowed_in(&format!("site/{name}"))
}

/// The banned names and the vendor names in `text`, but a vendor name inside an occurrence of one of the `allowed` texts.
fn brands(text: &str, allowed: &[&str]) -> Result<(), String> {
    let lower = xtask::brands::without_allowed(text, allowed).to_ascii_lowercase();
    if let Some(term) = xtask::brands::BANNED_NAMES.iter().find(|term| lower.contains(**term)) {
        return Err(format!("banned name {term}"));
    }
    if let Some(term) = lower.split(|c: char| !c.is_ascii_alphanumeric()).find(|word| xtask::brands::DENY.contains(word)) {
        return Err(format!("banned name {term}"));
    }
    Ok(())
}

fn inspect_file(dir: &Path, name: &str, origins: &Origins, hashed: bool) -> Result<File, String> {
    brands(name, &[])?;
    let path = dir.join(name);
    let size = path.metadata().map_err(|e| e.to_string())?.len();
    // Bound reads, including photos whose embedded metadata is also public.
    if size > 32 * 1024 * 1024 {
        return Err("individual file exceeds the 32 MiB scan limit".into());
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let mut file = File { bytes: size, ..File::default() };
    brands(&String::from_utf8_lossy(&bytes), &allowed(name))?;
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "ico" | "avif" | "gif") {
        return Ok(file);
    }
    // A font is bytes, as a picture is: counted and scanned for names above, and never read as text.
    if is_font(name) {
        if extension != "woff2" || !name.starts_with(HASHED_DIR) {
            return Err(format!("a font is a .woff2 file in {HASHED_DIR}, named by the hash of its bytes"));
        }
        return Ok(file);
    }
    let source = if extension == "html" { html::utf8(&bytes)? } else { std::str::from_utf8(&bytes).map_err(|e| format!("expected UTF-8: {e}"))? };
    match extension {
        "html" => html::parse(name, source, origins, &mut file)?,
        "css" => file.assets = links::css(source)?,
        "webmanifest" | "json" => {
            let value = serde_json::from_str(source).map_err(|e| format!("invalid JSON: {e}"))?;
            links::json(&value, origins, extension == "webmanifest", &mut file)?;
        }
        "svg" => return Err("SVG assets are outside this site's image formats".into()),
        _ if name == "_headers" => {
            file.font_src = headers::check_pages(source, hashed)?.font_src;
        }
        _ => {}
    }
    links::hosts(source, origins, name)?;
    Ok(file)
}

fn inspect(dir: &Path, origins: &Origins, names: Vec<String>) -> Result<(usize, cache::Summary), String> {
    let mut files = BTreeMap::new();
    let all = names.clone();
    let hashed = names.iter().any(|name| name.starts_with(HASHED_DIR));
    let fonts = names.iter().any(|name| is_font(name));
    for name in names {
        let file = inspect_file(dir, &name, origins, hashed).map_err(|e| format!("{name}: {e}"))?;
        files.insert(name, file);
    }
    for required in ["index.html", "_headers"] {
        if !files.contains_key(required) {
            return Err(format!("missing {required}"));
        }
    }
    // Fonts are the site's own files, and the policy says so in its own directive: `default-src` would allow them, and a reader of `_headers` would not see it.
    if fonts && !files.get("_headers").is_some_and(|headers| headers.font_src) {
        return Err("the site has fonts, but the policy of /* in _headers has no font-src 'self'".into());
    }
    let mut assets = BTreeMap::new();
    for (name, file) in &files {
        // A link to the app is a link to another site (`app_url`): the check resolves it as one, and the app's own folder is checked with `--web`.
        for link in &file.links {
            target(&files, name, link, origins, false)?;
        }
        let mut resolved = Vec::new();
        for link in &file.assets {
            if let Some(local) = target(&files, name, link, origins, true)? {
                resolved.push(local);
            }
        }
        assets.insert(name.as_str(), resolved);
    }
    let mut count = 0;
    for page in files.keys().filter(|name| name.ends_with(".html")) {
        let mut pending = vec![page.as_str()];
        let mut loaded = BTreeSet::new();
        let mut total = 0u64;
        while let Some(name) = pending.pop() {
            if loaded.insert(name) {
                let file = files.get(name).ok_or_else(|| format!("{page}: missing asset {name}"))?;
                total = total.checked_add(file.bytes).ok_or("asset size overflow")?;
                if let Some(children) = assets.get(name) {
                    pending.extend(children.iter().map(String::as_str));
                }
            }
        }
        if total > PAGE_BUDGET {
            return Err(format!("{page}: {total} bytes of HTML and assets exceed the {PAGE_BUDGET}-byte budget"));
        }
        count += 1;
    }
    // What `_headers` does to every file, read as Pages applies it: only a file named by its own hash is kept, and every page has its policy (`cache.rs`).
    let caching = cache::check_dir(dir, &all, cache::Kind::Pages)?;
    Ok((count, caching))
}

fn target(files: &BTreeMap<String, File>, name: &str, url: &str, origins: &Origins, asset: bool) -> Result<Option<String>, String> {
    let resolved = links::resolve(name, url, origins).map_err(|e| format!("{name}: {url}: {e}"))?;
    let Some((local, fragment)) = resolved else {
        return if asset { Err(format!("{name}: external asset {url}")) } else { Ok(None) };
    };
    let file = match files.get(&local) {
        Some(file) => file,
        None => return Err(format!("{name}: missing {url} (resolves to {local})")),
    };
    if local.ends_with(".html") && !fragment.is_empty() && !file.ids.contains(&fragment) {
        return Err(format!("{name}: {url} points to missing #{fragment} in {local}"));
    }
    Ok(Some(local))
}

/// The pages' check, for a site with this domain and the app at an address of its own that no test page names.
#[cfg(test)]
fn check(dir: &Path, domain: &str) -> Result<(), String> {
    inspect(dir, &Origins::new(domain, "https://app.example.invalid"), inventory(dir)?).map(|_| ())
}
