//! The web app's module in parts, and the files the app fetches: what `cargo xtask site --web` makes of Trunk's build, and what its check holds the result to.
//!
//! **Why parts.** Cloudflare Pages refuses a file over 25 MiB, and a visitor waits for the whole module before the first frame. The module is cut into parts of at most
//! [`PART_LIMIT`] (8 MiB), each named by the hash of its bytes (`parts/diacraft-web-1-0123456789abcdef.wasm`: the `.wasm` ending is what makes a CDN compress it), and served
//! with a cache header that says it never changes. The loader (`apps/diacraft-web/loader.js`, which becomes `start.js`) fetches the parts in parallel, joins them in order into
//! one stream, and hands that to the program's start, so that the browser compiles while the bytes still arrive.
//!
//! **What else is made.** The data the module does not carry (the splash picture, the cover, the licence list: `diacraft_ui_egui::Asset`) is copied into `assets/` under a name
//! with its hash. `manifest.json` names every part and asset with its size and SHA-256, and the start script of the program (the glue Trunk made). The page preloads the manifest
//! and the parts, so that they start to come before the loader has run.
//!
//! **What the check holds** ([`check`]): the manifest is the version this loader reads; each part is a file of `parts/` of the size the manifest says, at most [`PART_LIMIT`], named
//! by its hash, and its bytes have that hash; the parts joined are the module the manifest's hash names, and the module is a WebAssembly module with no shared memory, at most
//! [`MODULE_BUDGET`]; each asset is there by its name and hash and within its limit; nothing else is in `parts/` or `assets/`; and the page preloads exactly the manifest and the parts.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The most a part may be: what the loader reads and the check holds. A CDN's limit for a file is 25 MiB, and a part this size is fetched as one request.
pub(super) const PART_LIMIT: u64 = 8 * 1024 * 1024;

/// The most parts a module is cut into, and the most bytes of module the loader and this tool handle in all (the loader refuses more than 64 MiB).
pub(super) const MAX_PARTS: usize = 16;
pub(super) const MAX_MODULE: u64 = 64 * 1024 * 1024;

/// The most the module may be in all: room over what it is when this was written (docs/website.md, "The web app"), so that growth is seen when it happens. The limit of a file
/// (25 MiB) is not what it is held to any more, since it is in parts: the cost is the time a visitor waits.
pub(super) const MODULE_BUDGET: u64 = 19 * 1024 * 1024;

/// The most bytes of manifest anything reads.
pub(super) const MAX_MANIFEST: u64 = 64 * 1024;

/// The folders of the app that hold the parts and the assets, the manifest, and the file in the repository that is the loader.
pub(super) const PARTS_DIR: &str = "parts";
pub(super) const ASSETS_DIR: &str = "assets";
pub(super) const MANIFEST: &str = "manifest.json";
pub(super) const LOADER_SOURCE: &str = "apps/diacraft-web/loader.js";

/// The assets, as (key in the manifest, the file in the repository, the stem and the extension of its name in the site, the most bytes the program takes of it). The keys and limits
/// are `diacraft_ui_egui::Asset`'s (`key` and `limit`); a test of the web crate holds them to this table.
pub(super) const ASSETS: [(&str, &str, &str, &str, u64); 3] = [
    ("splash", "assets/branding/diacraft-splash.png", "diacraft-splash", "png", 1 << 20),
    ("cover", "assets/branding/diacraft-cover.png", "diacraft-cover", "png", 8 << 20),
    ("licenses", "THIRD-PARTY-LICENSES-WEB.txt", "third-party-licenses-web", "txt", 2 << 20),
];

/// The SHA-256 of `bytes`, in lower-case hexadecimal.
pub(super) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The first sixteen hexadecimal digits of a digest: what a file's name carries.
pub(super) fn short(digest: &str) -> &str {
    digest.get(..16).unwrap_or(digest)
}

/// Where a module of `len` bytes is cut: parts of near-equal size, as few as keep each at most [`PART_LIMIT`], contiguous, and together exactly `0..len`. An empty module is one empty
/// part's worth (no part). More than [`MAX_PARTS`] parts is an error.
pub(super) fn plan(len: u64) -> Result<Vec<std::ops::Range<usize>>, String> {
    if len == 0 {
        return Err("the module is empty".into());
    }
    if len > MAX_MODULE {
        return Err(format!("the module is {len} bytes, and the loader reads at most {MAX_MODULE}"));
    }
    let count = len.div_ceil(PART_LIMIT);
    if count > MAX_PARTS as u64 {
        return Err(format!("the module would need {count} parts, and the most is {MAX_PARTS}"));
    }
    // Near-equal parts: each is the whole divided by the count, rounded up, and the last is what is left.
    let size = len.div_ceil(count);
    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < len {
        let end = start.saturating_add(size).min(len);
        ranges.push(
            usize::try_from(start).map_err(|_| "a part starts past what this machine addresses")?
                ..usize::try_from(end).map_err(|_| "a part ends past what this machine addresses")?,
        );
        start = end;
    }
    Ok(ranges)
}

/// A regular file of at most `most` bytes, read whole. A link, another kind of file, or a larger file is an error: the size is looked at before a byte is read.
pub(super) fn read_bounded(path: &Path, most: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    if meta.len() > most {
        return Err(format!("{}: {} bytes, and at most {most} may be read", path.display(), meta.len()));
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The one file of `app` with the extension `extension`, not counting `skip`: the build has exactly one module and one start script.
fn only_file(app: &Path, extension: &str, skip: &str) -> Result<String, String> {
    let mut found = Vec::new();
    for entry in app.read_dir().map_err(|e| format!("{}: {e}", app.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", app.display()))?;
        let name = entry.file_name().into_string().map_err(|_| format!("{}: a name that is not UTF-8", app.display()))?;
        let is_file = entry.file_type().map_err(|e| format!("{}: {e}", app.display()))?.is_file();
        if is_file && name != skip && name.rsplit_once('.').is_some_and(|(_, ext)| ext == extension) {
            found.push(name);
        }
    }
    match found.as_slice() {
        [one] => Ok(one.clone()),
        other => Err(format!("{}: expected one .{extension} file in the build, found {}", app.display(), other.len())),
    }
}

/// Makes the app of `app` (a copy of Trunk's build, its start script already moved into `start.js`) loadable in parts: cuts the module, copies the assets, writes the manifest and the
/// loader, and returns the page with its preloads for the manifest and the parts in place of Trunk's one preload of the module. `root` is the repository, where the assets and the loader are.
pub(super) fn finish(root: &Path, app: &Path, html: &str) -> Result<String, String> {
    let module_file = only_file(app, "wasm", "")?;
    let glue = only_file(app, "js", "start.js")?;
    let module_path = app.join(&module_file);
    let module = read_bounded(&module_path, MAX_MODULE)?;
    if module.get(..8) != Some(b"\0asm\x01\0\0\0".as_slice()) {
        return Err(format!("{}: not a WebAssembly module (version 1)", module_path.display()));
    }
    let ranges = plan(module.len() as u64)?;
    let parts_dir = app.join(PARTS_DIR);
    fs::create_dir_all(&parts_dir).map_err(|e| format!("{}: {e}", parts_dir.display()))?;
    let mut parts = Vec::new();
    for (n, range) in ranges.into_iter().enumerate() {
        let bytes = module.get(range).ok_or("a part is not inside the module")?;
        let digest = sha256(bytes);
        let name = format!("{PARTS_DIR}/diacraft-web-{}-{}.wasm", n + 1, short(&digest));
        fs::write(app.join(&name), bytes).map_err(|e| format!("{name}: {e}"))?;
        parts.push(json!({"file": name, "bytes": bytes.len(), "sha256": digest}));
    }
    // What the parts replace is not part of the site: a file over the limit of Pages must not be deployed.
    fs::remove_file(&module_path).map_err(|e| format!("{}: {e}", module_path.display()))?;
    let mut assets = serde_json::Map::new();
    let assets_dir = app.join(ASSETS_DIR);
    fs::create_dir_all(&assets_dir).map_err(|e| format!("{}: {e}", assets_dir.display()))?;
    for (key, source, stem, extension, limit) in ASSETS {
        let bytes = read_bounded(&root.join(source), limit)?;
        let digest = sha256(&bytes);
        let name = format!("{ASSETS_DIR}/{stem}-{}.{extension}", short(&digest));
        fs::write(app.join(&name), &bytes).map_err(|e| format!("{name}: {e}"))?;
        assets.insert(key.to_owned(), json!({"file": name, "bytes": bytes.len(), "sha256": digest}));
    }
    let manifest = json!({
        "version": 1,
        "glue": glue,
        "module": {"bytes": module.len(), "sha256": sha256(&module), "parts": parts},
        "assets": assets,
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| format!("the manifest: {e}"))? + "\n";
    fs::write(app.join(MANIFEST), &text).map_err(|e| format!("{MANIFEST}: {e}"))?;
    // The loader, in place of the inline script Trunk's page carried.
    let loader = read_bounded(&root.join(LOADER_SOURCE), super::app::MAX_TEXT)?;
    fs::write(app.join("start.js"), loader).map_err(|e| format!("start.js: {e}"))?;
    let files: Vec<String> = parts.iter().filter_map(|part| part.get("file").and_then(Value::as_str).map(str::to_owned)).collect();
    preloads(html, &files)
}

/// The tags that make a browser start to fetch the manifest and the parts as soon as it has read the page: `<link rel="preload" as="fetch">`, which the loader's own fetches then
/// use (a request of the same address, the same mode and the same credentials is answered from the preload). Trunk's preload of the whole module goes, since the module is no file.
pub(super) fn preload_tags(files: &[String]) -> String {
    let mut tags = format!("<link rel=\"preload\" href=\"./{MANIFEST}\" as=\"fetch\" crossorigin=\"anonymous\">");
    for file in files {
        tags.push_str(&format!("<link rel=\"preload\" href=\"./{file}\" as=\"fetch\" crossorigin=\"anonymous\">"));
    }
    tags
}

/// `html` with every preload of a `.wasm` taken out and [`preload_tags`] put before the end of the head. One pass over the page: it is lower-cased once (ASCII only, so every offset is the same in
/// both), and each tag is found from where the last ended, so the work grows with the page and not with the page times its tags.
pub(super) fn preloads(html: &str, files: &[String]) -> Result<String, String> {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 512);
    let mut at = 0usize;
    while let Some(found) = next_tag(&lower, "<link", at) {
        let end = lower.get(found..).and_then(|rest| rest.find('>')).map(|n| found + n).ok_or("an unterminated <link> tag")?;
        out.push_str(html.get(at..found).ok_or("a tag is not inside the page")?);
        let tag = html.get(found..=end).ok_or("a tag is not inside the page")?;
        let tag_lower = lower.get(found..=end).unwrap_or("");
        if !(tag_lower.contains("rel=\"preload\"") && tag_lower.contains(".wasm")) {
            out.push_str(tag);
        }
        at = end + 1;
    }
    out.push_str(html.get(at..).ok_or("the page ends inside a tag")?);
    let head_end = out.to_ascii_lowercase().find("</head>").ok_or("the page has no </head>")?;
    out.insert_str(head_end, &preload_tags(files));
    Ok(out)
}

/// The place, at or after `from`, of the next `tag` (`<link`: a tag and not a longer word) in `lower`, which is a page in lower case.
fn next_tag(lower: &str, tag: &str, from: usize) -> Option<usize> {
    let mut from = from;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find(tag)) {
        let start = from + at;
        let next = lower.get(start + tag.len()..).and_then(|rest| rest.chars().next());
        if next.is_some_and(|c| c.is_ascii_whitespace() || c == '>') {
            return Some(start);
        }
        from = start + tag.len();
    }
    None
}

/// What the check found of the module: the sizes of its parts and of the whole.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Module {
    pub(super) bytes: u64,
    pub(super) parts: Vec<u64>,
}

fn str_of<'a>(value: &'a Value, key: &str, what: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| format!("{MANIFEST}: {what} has no \"{key}\""))
}

fn u64_of(value: &Value, key: &str, what: &str) -> Result<u64, String> {
    value.get(key).and_then(Value::as_u64).ok_or_else(|| format!("{MANIFEST}: {what} has no whole number \"{key}\""))
}

fn is_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Checks the parts, the assets and the manifest of `app` against each other and against the limits, as the module doc says. `files` are the files of `app` with their sizes, `page` is its
/// start page. Returns what the module is.
pub(super) fn check(app: &Path, files: &[(String, u64)], page: &str) -> Result<Module, String> {
    let size_of = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, bytes)| *bytes);
    let text = String::from_utf8(read_bounded(&app.join(MANIFEST), MAX_MANIFEST).map_err(|e| format!("{MANIFEST}: {e}"))?)
        .map_err(|_| format!("{MANIFEST}: not UTF-8"))?;
    super::brands(&text, &[]).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|e| format!("{MANIFEST}: not JSON ({e})"))?;
    if manifest.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{MANIFEST}: not the version (1) the loader reads"));
    }
    let glue = str_of(&manifest, "glue", "the manifest")?;
    if !glue.ends_with(".js") || glue.contains('/') || glue == "start.js" || size_of(glue).is_none() {
        return Err(format!("{MANIFEST}: the start script `{glue}` is not a JavaScript file of the app's folder"));
    }
    let module = manifest.get("module").ok_or_else(|| format!("{MANIFEST}: no \"module\""))?;
    let module_bytes = u64_of(module, "bytes", "the module")?;
    let module_digest = str_of(module, "sha256", "the module")?;
    let listed = module.get("parts").and_then(Value::as_array).ok_or_else(|| format!("{MANIFEST}: the module has no \"parts\""))?;
    if listed.is_empty() || listed.len() > MAX_PARTS {
        return Err(format!("{MANIFEST}: {} parts; between 1 and {MAX_PARTS} are allowed", listed.len()));
    }
    if module_bytes > MODULE_BUDGET {
        return Err(format!(
            "{MANIFEST}: the module is {module_bytes} bytes, over the budget of {MODULE_BUDGET} (docs/website.md, \"The web app\", says what to do)"
        ));
    }
    let mut joined: Vec<u8> = Vec::new();
    joined
        .try_reserve_exact(usize::try_from(module_bytes).map_err(|_| "the module's size does not fit this machine")?)
        .map_err(|_| "no room for the module")?;
    let mut sizes = Vec::new();
    let mut names = Vec::new();
    for (n, part) in listed.iter().enumerate() {
        let file = str_of(part, "file", "a part")?;
        let bytes = u64_of(part, "bytes", "a part")?;
        let digest = str_of(part, "sha256", "a part")?;
        if !is_digest(digest) || !is_digest(module_digest) {
            return Err(format!("{MANIFEST}: part {} has a digest that is not 64 hexadecimal digits", n + 1));
        }
        if bytes > PART_LIMIT {
            return Err(format!("{file}: {bytes} bytes, over the limit of {PART_LIMIT} for a part"));
        }
        if file != format!("{PARTS_DIR}/diacraft-web-{}-{}.wasm", n + 1, short(digest)) {
            return Err(format!(
                "{MANIFEST}: part {} is named `{file}`, and its name must be `{PARTS_DIR}/diacraft-web-{}-` and the first 16 digits of its hash",
                n + 1,
                n + 1
            ));
        }
        match size_of(file) {
            Some(on_disk) if on_disk == bytes => {}
            Some(on_disk) => return Err(format!("{file}: {on_disk} bytes on disk, and the manifest says {bytes}")),
            None => return Err(format!("{MANIFEST}: part {} is `{file}`, which is not in the app's folder", n + 1)),
        }
        let content = read_bounded(&app.join(file), PART_LIMIT)?;
        if sha256(&content) != digest {
            return Err(format!("{file}: its bytes do not have the hash the manifest and its name give"));
        }
        if joined.len().saturating_add(content.len()) as u64 > module_bytes {
            return Err(format!("{MANIFEST}: the parts are more than the {module_bytes} bytes of the module"));
        }
        joined.extend_from_slice(&content);
        sizes.push(bytes);
        names.push(file.to_owned());
    }
    if joined.len() as u64 != module_bytes {
        return Err(format!("{MANIFEST}: the parts add up to {} bytes, and the module is {module_bytes}", joined.len()));
    }
    if sha256(&joined) != module_digest {
        return Err(format!("{MANIFEST}: the parts joined are not the module whose hash it gives"));
    }
    super::app::check_wasm_bytes(&joined).map_err(|e| format!("the module (the parts joined): {e}"))?;
    // Nothing is in the two folders that the manifest does not name.
    let mut named: Vec<String> = names.clone();
    let assets = manifest.get("assets").and_then(Value::as_object).ok_or_else(|| format!("{MANIFEST}: no \"assets\""))?;
    if assets.len() != ASSETS.len() {
        return Err(format!("{MANIFEST}: {} assets, and the program fetches {}", assets.len(), ASSETS.len()));
    }
    for (key, _, stem, extension, limit) in ASSETS {
        let entry = assets.get(key).ok_or_else(|| format!("{MANIFEST}: no asset \"{key}\""))?;
        let file = str_of(entry, "file", key)?;
        let bytes = u64_of(entry, "bytes", key)?;
        let digest = str_of(entry, "sha256", key)?;
        if !is_digest(digest) || file != format!("{ASSETS_DIR}/{stem}-{}.{extension}", short(digest)) {
            return Err(format!(
                "{MANIFEST}: the asset \"{key}\" is `{file}`, and its name must be `{ASSETS_DIR}/{stem}-` and the first 16 digits of its hash `.{extension}`"
            ));
        }
        if bytes == 0 || bytes > limit {
            return Err(format!("{file}: {bytes} bytes, and the program takes at most {limit} of \"{key}\""));
        }
        match size_of(file) {
            Some(on_disk) if on_disk == bytes => {}
            Some(on_disk) => return Err(format!("{file}: {on_disk} bytes on disk, and the manifest says {bytes}")),
            None => return Err(format!("{MANIFEST}: the asset \"{key}\" is `{file}`, which is not in the app's folder")),
        }
        if sha256(&read_bounded(&app.join(file), limit)?) != digest {
            return Err(format!("{file}: its bytes do not have the hash the manifest and its name give"));
        }
        if extension == "txt" {
            let text = String::from_utf8(read_bounded(&app.join(file), limit)?).map_err(|_| format!("{file}: not UTF-8"))?;
            licence_names(&text).map_err(|e| format!("{file}: {e}"))?;
        }
        named.push(file.to_owned());
    }
    for (name, _) in files {
        let in_folder = name.starts_with(&format!("{PARTS_DIR}/")) || name.starts_with(&format!("{ASSETS_DIR}/"));
        if in_folder && !named.contains(name) {
            return Err(format!("{name}: in the app's folder, and the manifest does not name it"));
        }
    }
    check_preloads(page, &names)?;
    Ok(Module { bytes: module_bytes, parts: sizes })
}

/// The licence list is public text and is held to the names the rest of the site is, **except the lines the repository's own check keeps as the licences have them**
/// (`xtask::brands::NAME_EXCEPTIONS`: the copyright line of a licence is not ours to change).
fn licence_names(text: &str) -> Result<(), String> {
    for line in text.lines() {
        if xtask::brands::NAME_EXCEPTIONS.iter().any(|(file, kept)| *file == "THIRD-PARTY-LICENSES-WEB.txt" && line.contains(kept)) {
            continue;
        }
        super::brands(line, &[])?;
    }
    Ok(())
}

/// [`check_preloads`], for the tests of the sibling module.
#[cfg(test)]
pub(super) fn check_preloads_for_test(page: &str, parts: &[String]) -> Result<(), String> {
    check_preloads(page, parts)
}

/// The page preloads the manifest and every part, and nothing that is not one of them as a fetch.
fn check_preloads(page: &str, parts: &[String]) -> Result<(), String> {
    let mut found: Vec<String> = Vec::new();
    // One pass, from where the last tag ended (the page is lower-cased once).
    let page_lower = page.to_ascii_lowercase();
    let mut at = 0usize;
    while let Some(start) = next_tag(&page_lower, "<link", at) {
        let end = page_lower.get(start..).and_then(|rest| rest.find('>')).map(|n| start + n).ok_or("an unterminated <link> tag")?;
        let tag = page.get(start..=end).unwrap_or("");
        let lower = page_lower.get(start..=end).unwrap_or("");
        if lower.contains("rel=\"preload\"")
            && let Some(href) = super::app::attribute(tag, "href")
        {
            if !lower.contains("as=\"fetch\"") || !lower.contains("crossorigin") {
                return Err(format!(
                    "a preload of `{href}` that is not `as=\"fetch\"` with `crossorigin`: the loader's fetch would not use it, and the file would come twice"
                ));
            }
            found.push(href.strip_prefix("./").unwrap_or(href).to_owned());
        }
        at = end + 1;
    }
    let mut wanted: Vec<String> = vec![MANIFEST.to_owned()];
    wanted.extend(parts.iter().cloned());
    found.sort();
    wanted.sort();
    if found != wanted {
        return Err(format!("the page preloads {found:?}, and it must preload the manifest and each part, {wanted:?}"));
    }
    Ok(())
}
