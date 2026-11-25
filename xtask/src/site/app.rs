//! The web app as a site of its own: the Trunk build of `apps/diacraft-web`, made by `cargo xtask site --web` into a folder with the app at its root (for the Pages project
//! of `https://diacraft.app`), beside the folder of the pages (for `https://diacraft.dev`), and the checks that both can be deployed to Cloudflare Pages.
//!
//! **Why the build is not in `site/`.** The app is a WebAssembly file of several megabytes: it is made by a command and never committed. `cargo xtask site --web` copies `site/` into the pages' folder
//! (`dist/site` unless `--out DIR` says another; the pages' links to the app take the address of `app_url` in `site.json`, or of `--app-url`/`DIACRAFT_APP_URL` for a preview), runs
//! `trunk build --release` in `apps/diacraft-web` (or takes a finished build with `--dist DIR`), and makes the app's folder (`dist/app` unless `--app-out DIR` says another) from it, and checks both.
//!
//! **One change to Trunk's page.** Trunk starts the app with an inline `<script type="module">`, and the app's content security policy (`apps/diacraft-web/_headers`) has no
//! `'unsafe-inline'`. The script is moved into a file beside the page (`start.js`), unchanged, and the page loads it by `src`. Nothing is written by hand: the file
//! is Trunk's own text.
//!
//! **The module is served in parts** (`parts.rs`). Trunk's one `.wasm` is cut into parts of at most 8 MiB, each named by its hash, and the program's data that
//! the module need not carry (the splash picture, the cover, the licence list) is copied beside them under hashed names. `manifest.json` names them all with their sizes and
//! SHA-256, and `start.js` is the loader (`apps/diacraft-web/loader.js`): it fetches the parts in parallel, joins them in order into one stream and starts the program from it.
//!
//! **The app's own headers.** `_headers` of the app's folder is the file `apps/diacraft-web/_headers`: the app's policy for every path and the rules that keep its hashed files for a year (`headers.rs`). The pages'
//! file is `site/_headers`, and has nothing of the app's: the two sites are two Pages projects, and a path is matched by the rules of its own site alone.
//!
//! **What the check holds** (`check`): the start page and every file it loads exist (and no other reference leaves the folder), nothing is inline (no script,
//! style element or style attribute), no file is over the Pages limit of 25 MiB and none over the budget below it (a part has its own, 8 MiB), the parts joined are a module
//! whose memory is not shared (a shared memory needs cross-origin isolation headers, which the app does not send) and whose size is within its budget, the manifest and
//! the files agree (`parts::check`), the JavaScript glue holds no network call and no `eval`, the page, script and style name no vendor (the module's own text is not
//! scanned: see `check_wasm_bytes`), the page links the web app manifest (`web_manifest.rs`: a window of its own) and the manifest and its icons are as the check holds them, `_headers` is the app's own
//! (`headers::check_app`), and `_headers` read as Pages applies it keeps for a year the files whose names are the SHA-256 of their bytes and no other, and gives every page the app's policy (`cache.rs`).
//!
//! **The names Trunk made** (`rehash`). Trunk hashes the glue, the style and the icon with its own function. `assemble` names them again by the first 16 digits of the SHA-256 of
//! their bytes, as the parts and the data are named, so that one rule (a name that is a hash of the file's own bytes) tells what `_headers` may keep for a year. The snippets of the glue
//! (`snippets/eframe-<hash>/inline0.js`) are not renamed: the hash in the folder's name is of a crate's name and version, and not of the file, so they are asked about on every visit
//! (280 bytes).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::settings::Settings;
use super::{cache, headers, parts, seo, web_manifest};

/// The folder the pages must not have. The app was the pages' `app/` folder until it was given a site of its own: a folder of this name in `site/` would be deployed at `https://diacraft.dev/app/`.
pub(super) const APP_DIR: &str = "app";

/// The file in the repository that is the app's `_headers`: the app's policy and its cache rules, for the Pages project of the app's own site.
pub(super) const HEADERS_SOURCE: &str = "apps/diacraft-web/_headers";

/// Cloudflare Pages refuses a file over 25 MiB.
pub(super) const PAGES_FILE_LIMIT: u64 = 25 * 1024 * 1024;

/// The most a file of the app may be here, a part of the module aside (that is held to [`parts::PART_LIMIT`], and all of them to [`parts::MODULE_BUDGET`]): room under the
/// limit of Pages, so that growth is seen when it happens and not when a deploy is refused. The largest is the cover, 2.3 MiB when this was written (docs/website.md,
/// "The web app").
pub(super) const FILE_BUDGET: u64 = 4 * 1024 * 1024;

/// The most files the app folder holds (Trunk makes about six, and the module is a few parts and the data three files).
const MAX_FILES: usize = 64;

/// The deepest folder the copy follows, and the most files it copies.
const MAX_DEPTH: usize = 8;
const MAX_COPIED: usize = 4096;

/// The largest start page and the largest piece of JavaScript this check reads and rewrites.
pub(super) const MAX_TEXT: u64 = 2 * 1024 * 1024;

/// The file the inline start script is moved to.
const START_JS: &str = "start.js";

/// The most inline scripts the rewrite moves (Trunk makes one).
const MAX_INLINE_SCRIPTS: usize = 8;

/// What `cargo xtask site --web` was asked.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Web {
    /// A finished `trunk build --release` folder to use, and no build.
    pub(super) dist: Option<PathBuf>,
    /// The folder to make for the pages; `dist/site` of the repository when none.
    pub(super) out: Option<PathBuf>,
    /// The folder to make for the app, which is at its root; `dist/app` of the repository when none.
    pub(super) app_out: Option<PathBuf>,
    /// The address the pages' links to the app take instead of `app_url` of `site.json`: a preview on this machine (`settings::app_override` says what it may be). `DIACRAFT_APP_URL` is read when this is none.
    pub(super) app_url: Option<String>,
}

/// What the check found: each file of the app (its folder is its site's root) with its size, and their total, and the module, which is in parts.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) files: Vec<(String, u64)>,
    pub(super) total: u64,
    pub(super) module: parts::Module,
    /// What `_headers` does to every file of the app's folder (`cache.rs`).
    pub(super) caching: cache::Summary,
}

impl Report {
    /// The lines `cargo xtask site --web` prints.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.files.iter().map(|(name, bytes)| format!("site: app/{name}: {}", size(*bytes))).collect();
        lines.push(format!(
            "site: app: the module is {} in {} parts, none over {} (budget {}); the files other than parts are held to {} (Cloudflare Pages refuses a file over {})",
            size(self.module.bytes),
            self.module.parts.len(),
            size(parts::PART_LIMIT),
            size(parts::MODULE_BUDGET),
            size(FILE_BUDGET),
            size(PAGES_FILE_LIMIT)
        ));
        lines.push(format!("site: app: {} files, {} in all", self.files.len(), size(self.total)));
        lines.push(self.caching.line());
        lines
    }
}

/// `1234567` as `1.18 MiB (1234567 bytes)`.
fn size(bytes: u64) -> String {
    let (whole, hundredths) = size_parts(bytes);
    format!("{whole}.{hundredths:02} MiB ({bytes} bytes)")
}

/// A count of bytes as whole mebibytes and the hundredths of one past them, cut and not rounded: what [`size`] writes.
pub(crate) fn size_parts(bytes: u64) -> (u64, u64) {
    (bytes / 1024 / 1024, bytes % (1024 * 1024) * 100 / (1024 * 1024))
}

/// `cargo xtask site --web`: makes the pages' folder (`site/` with its links to the app at the address of this build) and checks it, builds the app (unless a build is given), makes the app's folder
/// from the build and checks it. `app_url` is the address a preview gives the app instead of the setting's. The pages come first: they are quick, and a fault in them is seen before the build, which is not.
pub(super) fn run(site: &Path, settings: &Settings, app_url: Option<String>, web: &Web) -> Result<(), String> {
    let root = site.parent().ok_or("the site folder has no parent")?;
    let out = web.out.clone().unwrap_or_else(|| root.join("dist").join("site"));
    let app_out = web.app_out.clone().unwrap_or_else(|| root.join("dist").join("app"));
    apart(&out, &app_out)?;
    // The pages take the address of the app of this build: the setting's, or the preview's.
    let pages = Settings { app_url: app_url.unwrap_or_else(|| settings.app_url.clone()), ..settings.clone() };
    assemble_pages(root, site, &out, &pages)?;
    let (count, caching) = super::check_pages(&out, &pages)?;
    println!("site: {count} pages in {} link the app at {}/ and are within {} bytes each", out.display(), pages.app_url, super::PAGE_BUDGET);
    println!("{}", caching.line());
    let dist = match &web.dist {
        Some(dist) => dist.clone(),
        None => {
            build(root)?;
            root.join("dist").join("web")
        }
    };
    assemble(root, &dist, &app_out, settings)?;
    let report = check(&app_out)?;
    for line in report.lines() {
        println!("{line}");
    }
    println!("site: the pages are in {}", out.display());
    println!("site: the app, at the root of its own site, is in {}", app_out.display());
    Ok(())
}

/// The pages' folder and the app's are two folders, one for each Pages project: neither is the other or inside it, or one would deploy the other's files.
fn apart(out: &Path, app_out: &Path) -> Result<(), String> {
    // The absolute path with `.` and `..` taken out in the text of it (the folders may not be there yet, so nothing is asked of the disk; a link in the way is `prepare`'s to find).
    let absolute = |path: &Path| -> Result<PathBuf, String> {
        let mut clean = PathBuf::new();
        for part in std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?.components() {
            match part {
                std::path::Component::ParentDir => {
                    clean.pop();
                }
                std::path::Component::CurDir => {}
                other => clean.push(other.as_os_str()),
            }
        }
        Ok(clean)
    };
    let (pages, app) = (absolute(out)?, absolute(app_out)?);
    if pages.starts_with(&app) || app.starts_with(&pages) {
        return Err(format!(
            "{} and {}: the pages and the app are two sites, each with a folder of its own, and neither may be the other or inside it (--out and --app-out)",
            out.display(),
            app_out.display()
        ));
    }
    Ok(())
}

/// Runs `trunk build --release --locked` in the web app's folder. Trunk makes the wasm, runs `wasm-bindgen` and `wasm-opt`, and writes `dist/web`.
fn build(root: &Path) -> Result<(), String> {
    let dir = root.join("apps").join("diacraft-web");
    eprintln!("$ trunk build --release --locked   (in apps/diacraft-web)");
    let status = Command::new("trunk").current_dir(&dir).args(["build", "--release", "--locked"]).status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "trunk is not installed: `cargo install --locked trunk` (docs/website.md, \"The web app\"), or give a finished build with --dist DIR"
                .to_owned()
        } else {
            format!("trunk: failed to spawn: {e}")
        }
    })?;
    if status.success() { Ok(()) } else { Err(format!("trunk build exited with {status}")) }
}

/// Copies `site/` into `out` and gives the copy's links to the app the address in `settings`, as `cargo xtask site --write` does for `site/` itself (`settings::apply`). With the setting's own address
/// the copy is `site/` as it is; a preview's makes the pages link the app on this machine.
pub(super) fn assemble_pages(root: &Path, site: &Path, out: &Path, settings: &Settings) -> Result<(), String> {
    if fs::symlink_metadata(site.join(APP_DIR)).is_ok() {
        return Err(format!(
            "{}: the pages hold no app: it is a site of its own, made by `cargo xtask site --web` into a folder of its own",
            site.join(APP_DIR).display()
        ));
    }
    prepare(root, out)?;
    let mut copied = 0usize;
    copy_tree(site, out, 0, &mut copied)?;
    // The copy's settings say what its pages say: a preview's `site.json` has the preview's address for the app.
    let path = out.join("site.json");
    if path.is_file() {
        let mut value: serde_json::Value = serde_json::from_str(&read_text(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        if value.get("app_url").and_then(serde_json::Value::as_str) != Some(settings.app_url.as_str()) {
            value["app_url"] = serde_json::Value::from(settings.app_url.as_str());
            let text = serde_json::to_string_pretty(&value).map_err(|e| format!("{}: {e}", path.display()))? + "\n";
            fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    super::settings::apply(out, settings, true)
}

/// Makes the app's folder from the Trunk build in `dist`: the build copied into `app` (an empty folder), the start script moved into a file, the files Trunk hashed named again, the module in parts with
/// the data beside it, the manifest, the loader and the app's own `_headers`.
pub(super) fn assemble(root: &Path, dist: &Path, app: &Path, settings: &Settings) -> Result<(), String> {
    let start = dist.join("index.html");
    if !start.is_file() {
        return Err(format!("{}: no index.html there: is that the output of `trunk build --release` in apps/diacraft-web?", dist.display()));
    }
    prepare(root, app)?;
    let mut copied = 0usize;
    copy_tree(dist, app, 0, &mut copied)?;
    let page = app.join("index.html");
    let html = read_text(&page)?;
    let (html, scripts) = externalise(&html)?;
    for (name, text) in scripts {
        let path = app.join(&name);
        if path.exists() {
            return Err(format!("{}: the build already has a file of that name", path.display()));
        }
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // The files Trunk hashed are named again by the SHA-256 of their bytes, so that every hashed name of the site is the hash of its own file.
    let html = rehash(app, &html)?;
    // The module in parts, the data beside it, the manifest, and the loader in place of Trunk's start script (`parts.rs`).
    let html = parts::finish(root, app, &html)?;
    // The tags a search engine and a social card read, from the page's own title and description and the settings: the app's own address, which a preview does not change.
    let html = seo::app_page(&html, settings)?;
    fs::write(&page, html).map_err(|e| format!("{}: {e}", page.display()))?;
    seo_files(root, app, settings)?;
    // The headers of the app's own site: its policy, and what is kept for a year.
    let headers = app.join("_headers");
    if fs::symlink_metadata(&headers).is_ok() {
        return Err(format!("{}: the build already has a file of that name", headers.display()));
    }
    let text = parts::read_bounded(&root.join(HEADERS_SOURCE), MAX_TEXT)?;
    fs::write(&headers, text).map_err(|e| format!("{}: {e}", headers.display()))?;
    Ok(())
}

/// The files beside the page that search engines and social cards ask for: `robots.txt`, `sitemap.xml` and the share picture, a copy of the pages' own. Trunk makes none of them, and a
/// file of those names in the build is an error, as it is for the headers.
fn seo_files(root: &Path, app: &Path, settings: &Settings) -> Result<(), String> {
    let picture = parts::read_bounded(&root.join("site").join("assets").join("social.jpg"), FILE_BUDGET)?;
    for (name, bytes) in
        [("robots.txt", seo::app_robots(settings).into_bytes()), ("sitemap.xml", seo::app_sitemap(settings).into_bytes()), (seo::APP_SOCIAL, picture)]
    {
        let path = app.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(format!("{}: the build already has a file of that name", path.display()));
        }
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// The most files at the app's root that [`rehash`] renames: Trunk makes three (the glue, the style, the icon).
const MAX_RENAMED: usize = 8;

/// The kinds of file Trunk hashes and puts at the root, whose names [`rehash`] changes: not `.wasm` (the module is cut into parts), `.html`, `.json` or `.webmanifest`.
const HASHED_KINDS: [&str; 8] = ["js", "css", "png", "jpg", "jpeg", "webp", "ico", "gif"];

/// `Some((stem, extension))` for a name Trunk made with its own hash: `<stem>-<12 to 16 lower-case hexadecimal digits>.<extension>` of a kind in [`HASHED_KINDS`]. Trunk writes 16 digits for what
/// it copies and the digits of the hash as they come for the glue (15 when the first is a zero).
pub(super) fn trunk_hashed(name: &str) -> Option<(&str, &str)> {
    let (base, extension) = name.rsplit_once('.')?;
    if !HASHED_KINDS.contains(&extension) {
        return None;
    }
    let (stem, digits) = base.rsplit_once('-')?;
    let hex = (12..=16).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (hex && !stem.is_empty()).then_some((stem, extension))
}

/// Names the files Trunk hashed at the root of `app` again, by the first 16 digits of the SHA-256 of their bytes, and returns `html` with the names changed. **Why.** A file may be kept for a year
/// by `_headers` only if its name is a hash of its own bytes, and the check (`cache.rs`) wants to see that it is. Trunk's hash is its own (a different function, and its digits can be fewer),
/// which nothing here can check, so the name is made again by the one function every other hashed file of the site uses (`parts.rs`). The page's `integrity` attributes are of the bytes and stay.
/// A file Trunk hashed that the page does not link is an error: the build is not what this expects.
fn rehash(app: &Path, html: &str) -> Result<String, String> {
    let mut renames: Vec<(String, String)> = Vec::new();
    for entry in app.read_dir().map_err(|e| format!("{}: {e}", app.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", app.display()))?;
        let name = entry.file_name().into_string().map_err(|_| format!("{}: a name that is not UTF-8", app.display()))?;
        if !entry.file_type().map_err(|e| format!("{}: {e}", app.join(&name).display()))?.is_file() {
            continue;
        }
        let Some((stem, extension)) = trunk_hashed(&name) else { continue };
        if renames.len() >= MAX_RENAMED {
            return Err(format!("{}: more than {MAX_RENAMED} files with a hash in their names at the root: Trunk makes three", app.display()));
        }
        let bytes = parts::read_bounded(&app.join(&name), FILE_BUDGET)?;
        renames.push((name.clone(), format!("{stem}-{}.{extension}", parts::short(&parts::sha256(&bytes)))));
    }
    renames.sort();
    let mut html = html.to_owned();
    for (old, new) in renames {
        if old == new {
            continue;
        }
        if !html.contains(&old) {
            return Err(format!("app/{old}: has a hash in its name and the page does not link it: Trunk's output is not what `site --web` expects"));
        }
        let target = app.join(&new);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(format!("app/{old} would be named {new}, which is a file already: two files with the same name and the same bytes"));
        }
        fs::rename(app.join(&old), &target).map_err(|e| format!("app/{old}: {e}"))?;
        html = html.replace(&old, &new);
    }
    Ok(html)
}

/// Makes `out` an empty folder: it is made when it is not there, and an old one is cleared only when it is under `dist/` of the repository (a folder this
/// command made) or empty already. Another full folder is the person's: it is not touched.
fn prepare(root: &Path, out: &Path) -> Result<(), String> {
    match fs::symlink_metadata(out) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", out.display())),
        Ok(meta) if meta.is_dir() => {
            let empty = out.read_dir().map_err(|e| format!("{}: {e}", out.display()))?.next().is_none();
            if !empty {
                let inside = out
                    .canonicalize()
                    .ok()
                    .zip(root.join("dist").canonicalize().ok())
                    .is_some_and(|(out, dist)| out.starts_with(&dist) && out != dist);
                if !inside {
                    return Err(format!(
                        "{}: not empty, and not a folder under dist/ that this command made: remove it or choose another with --out or --app-out",
                        out.display()
                    ));
                }
                fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
            }
        }
        Ok(_) => return Err(format!("{}: not a folder", out.display())),
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))
}

/// Copies the regular files under `from` into `to`. A link or another kind of file is an error, as in the checks of the site, and so is a tree that is too
/// deep or holds too many files.
fn copy_tree(from: &Path, to: &Path, depth: usize, copied: &mut usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{}: folders nested more than {MAX_DEPTH} deep", from.display()));
    }
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in from.read_dir().map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        let target = to.join(entry.file_name());
        // Trunk's `.trunk-lock` is its own: it is not part of the site (and a file whose name starts with a dot is not served as part of a page anyway).
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            copy_tree(&path, &target, depth + 1, copied)?;
        } else if kind.is_file() {
            *copied += 1;
            if *copied > MAX_COPIED {
                return Err(format!("{}: more than {MAX_COPIED} files", from.display()));
            }
            fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            return Err(format!("{}: only regular files may be deployed", path.display()));
        }
    }
    Ok(())
}

/// A text file of at most [`MAX_TEXT`] bytes.
fn read_text(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.len() > MAX_TEXT {
        return Err(format!("{}: expected a file of at most {MAX_TEXT} bytes", path.display()));
    }
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{}: expected UTF-8", path.display()))
}

/// Moves each inline `<script>` of `html` into a file, and has the page load the file by `src`. Returns the new page and the files as (name, text). The text
/// of a script is unchanged. A page with no inline script comes back as it was.
pub(super) fn externalise(html: &str) -> Result<(String, Vec<(String, String)>), String> {
    let mut out = String::with_capacity(html.len());
    let mut files = Vec::new();
    let mut rest = html;
    while let Some(at) = find_script_open(rest) {
        let (before, tag_and_after) = rest.split_at(at);
        let open_end = tag_and_after.find('>').ok_or("an unterminated <script> tag")?;
        let (open, after_open) = tag_and_after.split_at(open_end + 1);
        out.push_str(before);
        // A block of data is no script: it stays in the page as it is (the build then refuses a page that has one, since it makes its own).
        if attribute(open, "type") == Some("application/ld+json") {
            let close = find_ci(after_open, "</script>").ok_or("a block of data with no end tag")? + "</script>".len();
            out.push_str(open);
            out.push_str(after_open.get(..close).ok_or("a block of data is not inside the page")?);
            rest = after_open.get(close..).unwrap_or("");
            continue;
        }
        if attribute(open, "src").is_some() {
            // Already a file: kept as it is, with its own end tag.
            let close = find_ci(after_open, "</script>").ok_or("a <script src> with no end tag")?;
            let (inner, tail) = after_open.split_at(close);
            if !inner.trim().is_empty() {
                return Err("a <script> with both a src and a body".into());
            }
            out.push_str(open);
            out.push_str("</script>");
            rest = tail.get("</script>".len()..).unwrap_or("");
            continue;
        }
        let close = find_ci(after_open, "</script>").ok_or("an inline <script> with no end tag")?;
        let (body, tail) = after_open.split_at(close);
        if files.len() >= MAX_INLINE_SCRIPTS {
            return Err(format!("more than {MAX_INLINE_SCRIPTS} inline scripts"));
        }
        let name = if files.is_empty() { START_JS.to_owned() } else { format!("start-{}.js", files.len() + 1) };
        let module = attribute(open, "type").is_some_and(|t| t == "module");
        out.push_str(&format!("<script{} src=\"./{name}\"></script>", if module { " type=\"module\"" } else { "" }));
        files.push((name, body.trim_matches(['\n', '\r']).to_owned() + "\n"));
        rest = tail.get("</script>".len()..).unwrap_or("");
    }
    out.push_str(rest);
    Ok((out, files))
}

/// The place of `needle` (lower case, ASCII) in `text`, whatever the case of the text.
fn find_ci(text: &str, needle: &str) -> Option<usize> {
    // ASCII lower-casing keeps every byte offset.
    text.to_ascii_lowercase().find(needle)
}

/// The place of the next `<script` tag name (not `<scriptfoo`), case-insensitively.
fn find_script_open(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|s| s.find("<script")) {
        let start = from + at;
        let next = lower.get(start + "<script".len()..).and_then(|s| s.chars().next());
        if next.is_some_and(|c| c == '>' || c.is_ascii_whitespace()) {
            return Some(start);
        }
        from = start + "<script".len();
    }
    None
}

/// The value of attribute `name` in the tag text `tag` (`<script type="module">`), when it has one in double or single quotes.
pub(super) fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|s| s.find(name)) {
        let start = from + at;
        from = start + name.len();
        let before_ok = start > 0 && lower.as_bytes().get(start - 1).is_some_and(|b| b.is_ascii_whitespace());
        let rest = tag.get(start + name.len()..).unwrap_or("").trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let value = rest.get(1..)?;
        let end = value.find(quote)?;
        if before_ok {
            return value.get(..end);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------------------- the checks

/// Checks the folder `app` that `assemble` made: the app, at the root of its own site. The pages are checked as the pages of `site/` are (`cargo xtask site`);
/// this holds what the app needs and what it must not have. A file is named `app/<its path>` in what is said of it, to say that it is a file of the app.
pub(super) fn check(app: &Path) -> Result<Report, String> {
    let files = list(app)?;
    if !files.iter().any(|(name, _)| name == "index.html") {
        return Err(format!("{}: no index.html", app.display()));
    }
    let mut total = 0u64;
    for (name, bytes) in &files {
        total = total.checked_add(*bytes).ok_or("the app's size overflows")?;
        if *bytes > PAGES_FILE_LIMIT {
            return Err(format!("app/{name}: {} is over the {} Cloudflare Pages allows a file", size(*bytes), size(PAGES_FILE_LIMIT)));
        }
        // A part of the module has a limit of its own, and the parts together a budget (`parts::check`).
        let (budget, what) =
            if name.starts_with("parts/") { (parts::PART_LIMIT, "the limit for a part of the module") } else { (FILE_BUDGET, "the budget") };
        if *bytes > budget {
            return Err(format!(
                "app/{name}: {} is over {what}, {}, which leaves room under the {} limit of Pages: see docs/website.md, \"The web app\", for what to do",
                size(*bytes),
                size(budget),
                size(PAGES_FILE_LIMIT)
            ));
        }
        super::brands(name, &[], xtask::brands::DENY).map_err(|e| format!("app/{name}: {e}"))?;
    }
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    let page = read_text(&app.join("index.html"))?;
    check_page(&page, &names).map_err(|e| format!("app/index.html: {e}"))?;
    // The web app manifest: the page links it, and it is the one file of its kind.
    web_manifest::linked(&page).map_err(|e| format!("app/index.html: {e}"))?;
    if !names.contains(&web_manifest::FILE) {
        return Err(format!("app/{}: the page links it and the build did not make it", web_manifest::FILE));
    }
    for (name, _) in &files {
        let path = app.join(name);
        match name.rsplit_once('.').map(|(_, ext)| ext) {
            Some("js" | "mjs") => {
                let text = read_text(&path)?;
                check_script(&text, &names).map_err(|e| format!("app/{name}: {e}"))?;
            }
            // The parts of the module are checked together, joined, by `parts::check`; a `.wasm` anywhere else is the module whole, which is no longer served.
            Some("wasm") if name.starts_with("parts/") => {}
            Some("wasm") => {
                return Err(format!(
                    "app/{name}: a WebAssembly file outside parts/: the module is served in parts of at most {}",
                    size(parts::PART_LIMIT)
                ));
            }
            Some("json") if name == parts::MANIFEST => {}
            Some("webmanifest") if name == web_manifest::FILE => {
                let text = read_text(&path)?;
                web_manifest::check(&text, &names, app).map_err(|e| format!("app/{name}: {e}"))?;
            }
            Some("txt") if name.starts_with("assets/") || name == "robots.txt" => {}
            Some("xml") if name == "sitemap.xml" => {}
            Some("css") => {
                let text = read_text(&path)?;
                check_style(&text, &names).map_err(|e| format!("app/{name}: {e}"))?;
            }
            Some("png" | "ico" | "webp" | "jpg" | "jpeg") => {}
            _ if name == "index.html" || name == "_headers" => {}
            _ => return Err(format!("app/{name}: a file of a kind the app does not use: say so here if it is one")),
        }
    }
    let module = parts::check(app, &files, &page)?;
    // What a search engine and a social card read: the page's tags and data, and the files beside it.
    let robots = read_text(&app.join("robots.txt")).map_err(|e| format!("app/robots.txt: {e}"))?;
    let sitemap = read_text(&app.join("sitemap.xml")).map_err(|e| format!("app/sitemap.xml: {e}"))?;
    super::brands(&robots, &[], xtask::brands::DENY).map_err(|e| format!("app/robots.txt: {e}"))?;
    super::brands(&sitemap, &[], xtask::brands::DENY).map_err(|e| format!("app/sitemap.xml: {e}"))?;
    seo::check_app(&page, &robots, &sitemap, &names).map_err(|e| format!("app/index.html: {e}"))?;
    let headers = read_text(&app.join("_headers")).map_err(|e| format!("_headers: {e}"))?;
    super::brands(&headers, &[], xtask::brands::DENY).map_err(|e| format!("_headers: {e}"))?;
    headers::check_app(&headers).map_err(|e| format!("_headers: {e}"))?;
    // What Pages would send for every file of the app's folder: what is kept for a year is named by its hash and nothing else is, and every page has the app's policy.
    let caching = cache::check_dir(app, &super::inventory(app)?, cache::Kind::App)?;
    Ok(Report { files, total, module, caching })
}

/// The regular files under `app/` with their sizes, by their path from `app/` with `/` separators, sorted. A link or another kind of file, a folder nested
/// too deep, or too many files is an error.
fn list(app: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut files = Vec::new();
    list_into(app, "", 0, &mut files)?;
    files.sort();
    Ok(files)
}

fn list_into(folder: &Path, prefix: &str, depth: usize, files: &mut Vec<(String, u64)>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("app/{prefix}: folders nested more than {MAX_DEPTH} deep"));
    }
    for entry in folder.read_dir().map_err(|e| format!("{}: {e}", folder.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
        let name = entry.file_name().into_string().map_err(|_| format!("{}: a name that is not UTF-8", folder.display()))?;
        let kind = entry.file_type().map_err(|e| format!("app/{prefix}{name}: {e}"))?;
        if kind.is_dir() {
            list_into(&entry.path(), &format!("{prefix}{name}/"), depth + 1, files)?;
        } else if kind.is_file() {
            let bytes = entry.metadata().map_err(|e| format!("app/{prefix}{name}: {e}"))?.len();
            files.push((format!("{prefix}{name}"), bytes));
            if files.len() > MAX_FILES {
                return Err(format!("app/: more than {MAX_FILES} files"));
            }
        } else {
            return Err(format!("app/{prefix}{name}: only regular files may be deployed"));
        }
    }
    Ok(())
}

/// What the start page may hold and must load. `names` are the files beside it.
fn check_page(html: &str, names: &[&str]) -> Result<(), String> {
    super::brands(html, &[], xtask::brands::DENY)?;
    let lower = html.to_ascii_lowercase();
    // Every script tag of the page, not only the first, loads a file.
    let mut rest = html;
    while let Some(at) = find_script_open(rest) {
        let tag = rest.get(at..).and_then(|s| s.split('>').next()).unwrap_or("");
        // A block of data (`type="application/ld+json"`) is no script: a browser does not run it, and the policy does not see it.
        if attribute(tag, "src").is_none() && attribute(tag, "type") != Some("application/ld+json") {
            return Err("an inline <script>: the content security policy has no 'unsafe-inline'".into());
        }
        rest = rest.get(at + "<script".len()..).unwrap_or("");
    }
    for banned in ["<style", " style=", "<base", "<iframe", "<object", "<embed", "<form", "javascript:", " onload=", " onerror=", " onclick="] {
        if lower.contains(banned) {
            return Err(format!("it holds `{banned}`, which the content security policy for the app does not allow"));
        }
    }
    for tag in lower.split('<').skip(1) {
        if !(tag.starts_with("script") || tag.starts_with("link")) {
            continue;
        }
        let tag = tag.split('>').next().unwrap_or("");
        // The canonical address is the app's own address, which a page names and loads nothing from.
        if attribute(tag, "rel") == Some("canonical") {
            continue;
        }
        for key in ["src", "href"] {
            if let Some(url) = attribute(tag, key) {
                local_file(url, names)?;
            }
        }
    }
    if !lower.contains("<canvas id=\"diacraft_canvas\"") {
        return Err("no <canvas id=\"diacraft_canvas\">: the app has nothing to draw on".into());
    }
    if !lower.contains("type=\"module\"") || !lower.contains(&format!("src=\"./{START_JS}\"")) {
        return Err(format!("it does not load {START_JS} as a module: the app would not start"));
    }
    if !lower.contains("rel=\"stylesheet\"") {
        return Err("it loads no stylesheet: the canvas would not be sized".into());
    }
    Ok(())
}

/// A reference in the page or a script: a file beside it (`./name`, or the bare name), and one that exists. Anything that leaves the folder is an error.
fn local_file(url: &str, names: &[&str]) -> Result<(), String> {
    let name = url.strip_prefix("./").unwrap_or(url);
    let path = name.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':']) || path.split('/').any(|part| part.is_empty() || part.starts_with('.'))
    {
        return Err(format!(
            "`{url}` is not a file of the app's folder: the app is served from a folder of its own and loads nothing from outside it"
        ));
    }
    if names.contains(&path) { Ok(()) } else { Err(format!("`{url}` names a file the build did not make")) }
}

/// The JavaScript: what Trunk makes (`wasm-bindgen`'s glue and the start script). It may not reach the network by any means of its own, and may not use
/// `eval`, which the policy does not allow either. Every module it imports from the folder is a file of the build.
fn check_script(text: &str, names: &[&str]) -> Result<(), String> {
    super::brands(text, &[], xtask::brands::DENY)?;
    for banned in
        ["XMLHttpRequest", "WebSocket", "sendBeacon", "EventSource", "importScripts", "new Function", "eval(", "RTCPeerConnection", "WebTransport"]
    {
        if text.contains(banned) {
            return Err(format!("it holds `{banned}`: the app makes no network request of its own, and the policy allows no eval"));
        }
    }
    // A string that is an address of another site: what the glue would load or fetch from there. (A link in a comment or a message is not one.)
    for quote in ['\'', '"', '`'] {
        for scheme in ["http://", "https://"] {
            if text.contains(&format!("{quote}{scheme}")) {
                return Err(format!("it holds a string that starts with `{scheme}`: the app loads nothing from another site"));
            }
        }
        // `//host/path` is an address of another site too.
        let relative = format!("{quote}//");
        if text
            .match_indices(&relative)
            .any(|(at, _)| text.get(at + relative.len()..).and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_alphanumeric()))
        {
            return Err("it holds a string that starts with `//` and a host: the app loads nothing from another site".into());
        }
    }
    // Every module it imports from the folder, by `import ... from './x'`, `import './x'`, `import('./x')` or `new URL('./x', import.meta.url)`, is a file of the build.
    for marker in ["from", "import", "URL("] {
        for (at, _) in text.match_indices(marker) {
            let before_ok = at == 0
                || text.get(..at).and_then(|t| t.chars().next_back()).is_some_and(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '$' && c != '.');
            let rest = text.get(at + marker.len()..).unwrap_or("").trim_start().trim_start_matches('(').trim_start();
            let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"')) else { continue };
            let Some(piece) = rest.get(1..).and_then(|r| r.split(quote).next()) else { continue };
            if before_ok && (piece.starts_with("./") || piece.starts_with("../")) {
                local_file(piece, names)?;
            }
        }
    }
    Ok(())
}

/// A stylesheet of the app: no import and no address.
fn check_style(text: &str, names: &[&str]) -> Result<(), String> {
    super::brands(text, &[], xtask::brands::DENY)?;
    for url in super::links::css(text)? {
        local_file(&url, names)?;
    }
    if text.contains("@import") || text.contains("//") && text.contains("://") {
        return Err("it imports or names another address".into());
    }
    Ok(())
}

/// The WebAssembly module (the parts joined): a module (not an error page that was saved under its name), whose memory is not shared.
///
/// **Its text is not scanned for names.** The pages, the script and the style are public text and are held to the rules of the site. The module is the program:
/// it carries the names the file formats it reads require (content types and namespace addresses of the files it opens, which `AGENTS.md` allows as names needed
/// for interoperability) and the licence texts of its libraries with their copyright lines. The desktop downloads carry the same, and the repository's own check
/// (`cargo xtask brands`) holds the source they come from.
pub(super) fn check_wasm_bytes(bytes: &[u8]) -> Result<(), String> {
    if bytes.get(..8) != Some(b"\0asm\x01\0\0\0".as_slice()) {
        return Err("it is not a WebAssembly module (version 1)".into());
    }
    match wasm_shared_memory(bytes) {
        Ok(false) => Ok(()),
        Ok(true) => Err("it declares a shared memory (threads): the page would need cross-origin isolation headers (Cross-Origin-Opener-Policy and Cross-Origin-Embedder-Policy), which the site does not send".into()),
        Err(e) => Err(format!("its memory could not be read: {e}")),
    }
}

/// An unsigned LEB128 number at `*at` of `bytes` (at most ten bytes, 64 bits), moving `*at` past it. `None` for a number that is cut off or too long.
pub(crate) fn leb128(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in 0..10u32 {
        let byte = *bytes.get(*at)?;
        *at = at.checked_add(1)?;
        let part = u64::from(byte & 0x7f);
        // The tenth byte holds only the top bit of a 64-bit number.
        if shift == 9 && part > 1 {
            return None;
        }
        value |= part << (7 * shift);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Whether a WebAssembly module declares a shared memory, either in its memory section or as an import (what the threads proposal makes, and what needs
/// `SharedArrayBuffer`). Reads the import and memory sections only, with bounds checked at each step.
pub(crate) fn wasm_shared_memory(bytes: &[u8]) -> Result<bool, String> {
    const SHARED: u64 = 0x02;
    let cut = || "the module is cut off".to_owned();
    let mut at = 8usize;
    while at < bytes.len() {
        let id = *bytes.get(at).ok_or_else(cut)?;
        at += 1;
        let size = usize::try_from(leb128(bytes, &mut at).ok_or_else(cut)?).map_err(|_| "a section is too large".to_owned())?;
        let end = at.checked_add(size).filter(|end| *end <= bytes.len()).ok_or_else(cut)?;
        match id {
            // Imports: module, name, then the kind of the thing.
            2 => {
                let mut p = at;
                let count = leb128(bytes, &mut p).ok_or_else(cut)?;
                for _ in 0..count {
                    for _ in 0..2 {
                        let len = usize::try_from(leb128(bytes, &mut p).ok_or_else(cut)?).map_err(|_| cut())?;
                        p = p.checked_add(len).filter(|p| *p <= end).ok_or_else(cut)?;
                    }
                    let kind = *bytes.get(p).ok_or_else(cut)?;
                    p += 1;
                    match kind {
                        // A function: its type. A tag: an attribute byte and its type.
                        0 => {
                            leb128(bytes, &mut p).ok_or_else(cut)?;
                        }
                        4 => {
                            p += 1;
                            leb128(bytes, &mut p).ok_or_else(cut)?;
                        }
                        // A table: its element type, then limits.
                        1 => {
                            p += 1;
                            limits(bytes, &mut p, end)?;
                        }
                        2 => {
                            if limits(bytes, &mut p, end)? & SHARED != 0 {
                                return Ok(true);
                            }
                        }
                        // A global: its type and whether it can change.
                        3 => p += 2,
                        other => return Err(format!("an import of the kind {other}")),
                    }
                    if p > end {
                        return Err(cut());
                    }
                }
            }
            // Memories: limits each.
            5 => {
                let mut p = at;
                let count = leb128(bytes, &mut p).ok_or_else(cut)?;
                for _ in 0..count {
                    if limits(bytes, &mut p, end)? & SHARED != 0 {
                        return Ok(true);
                    }
                }
            }
            _ => {}
        }
        at = end;
    }
    Ok(false)
}

/// The limits of a memory or a table at `*at`: the flags byte, the minimum and, when the flags say so, the maximum. Returns the flags.
fn limits(bytes: &[u8], at: &mut usize, end: usize) -> Result<u64, String> {
    let cut = || "the module is cut off".to_owned();
    let flags = u64::from(*bytes.get(*at).ok_or_else(cut)?);
    *at += 1;
    leb128(bytes, at).ok_or_else(cut)?;
    if flags & 0x01 != 0 {
        leb128(bytes, at).ok_or_else(cut)?;
    }
    if *at > end { Err(cut()) } else { Ok(flags) }
}
