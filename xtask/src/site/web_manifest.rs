//! The web app manifest of the app: what lets Chrome and Edge open the app in a window of its own, where the keys a tab keeps for the browser (Ctrl+N, Ctrl+W) go to the page. The app is
//! the root of a site of its own, so its start page, scope and id are `/`.
//!
//! The file is `diacraft.webmanifest`, and not `manifest.json`, which is the module's list of parts. **Chrome installs a page with a manifest and no service worker**
//! (measured with `Page.getInstallabilityErrors` on Chrome 158: no error, and `beforeinstallprompt` is sent), so the app has none and the check holds it to that: nothing here
//! asks for a worker, a file handler, a share target or any other power. What the check holds: the file is JSON, its keys are the ones below and no others, the app's name and its start
//! page and scope are `/`, the display is `standalone` (a window with no tabs), the colours are `#rrggbb`, and the icons are PNG files of the folder, of the sizes they say, with
//! the 192 and the 512 an installable page needs. The page links it.

use std::path::Path;

use serde_json::Value;

/// The manifest's file name in the app's folder.
pub(super) const FILE: &str = "diacraft.webmanifest";

/// What the app's start page, scope and id are, as the browser reads them: the root of the app's site.
const APP_PATH: &str = "/";

/// The keys the manifest may have: everything that names the app, where it starts and how it looks, and nothing that asks the browser for more.
const KEYS: [&str; 10] = ["id", "name", "short_name", "description", "start_url", "scope", "display", "background_color", "theme_color", "icons"];

/// The sizes of icon an installable page needs.
const NEEDED: [u32; 2] = [192, 512];

/// The most icons a manifest may list.
const MAX_ICONS: usize = 8;

fn text<'a>(manifest: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, String> {
    match manifest.get(key) {
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(text),
        Some(_) => Err(format!("`{key}` must be text, and not empty")),
        None => Err(format!("it has no `{key}`")),
    }
}

/// A colour of the manifest: `#rrggbb`.
fn colour(manifest: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    let value = text(manifest, key)?;
    if value.len() == 7 && value.starts_with('#') && value.get(1..).is_some_and(|d| d.chars().all(|c| c.is_ascii_hexdigit())) {
        Ok(())
    } else {
        Err(format!("`{key}` is `{value}`: a colour is written #rrggbb"))
    }
}

/// The width and height a PNG's header says (its first chunk is the header, `IHDR`), or `None` for bytes that are not one.
pub(super) fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let number = |at: usize| bytes.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes);
    Some((number(16)?, number(20)?))
}

/// Checks the manifest `text`. `names` are the files of the app's folder, and `folder` is where they are (an icon's header is read to see its size).
pub(super) fn check(text_of_file: &str, names: &[&str], folder: &Path) -> Result<(), String> {
    super::brands(text_of_file, &[])?;
    let value: Value = serde_json::from_str(text_of_file).map_err(|e| format!("it is not JSON: {e}"))?;
    let Value::Object(manifest) = &value else { return Err("it is not a JSON object".into()) };
    if let Some(key) = manifest.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!(
            "it has the key `{key}`: the app asks the browser for what makes it installable and nothing more (a service worker, a file handler, a share target or a shortcut are not part of it)"
        ));
    }
    for key in ["name", "short_name", "description"] {
        text(manifest, key)?;
    }
    for key in ["id", "start_url", "scope"] {
        let value = text(manifest, key)?;
        if value != APP_PATH {
            return Err(format!(
                "`{key}` is `{value}`, and must be `{APP_PATH}`: the app is served from the root of a site of its own, and takes no page beside it"
            ));
        }
    }
    let display = text(manifest, "display")?;
    if display != "standalone" {
        return Err(format!(
            "`display` is `{display}`, and must be `standalone`: a window of its own with no tabs, where the browser passes its keys to the page"
        ));
    }
    colour(manifest, "background_color")?;
    colour(manifest, "theme_color")?;
    let icons = match manifest.get("icons") {
        Some(Value::Array(icons)) if !icons.is_empty() && icons.len() <= MAX_ICONS => icons,
        Some(Value::Array(icons)) => return Err(format!("`icons` has {} entries: from 1 to {MAX_ICONS}", icons.len())),
        _ => return Err("it has no `icons` list".into()),
    };
    let mut seen = Vec::new();
    for icon in icons {
        let Value::Object(icon) = icon else { return Err("an icon is not an object".into()) };
        if let Some(key) = icon.keys().find(|k| !["src", "sizes", "type", "purpose"].contains(&k.as_str())) {
            return Err(format!("an icon has the key `{key}`"));
        }
        let src = text(icon, "src")?;
        let file = src.strip_prefix("./").unwrap_or(src);
        if file.is_empty() || file.contains(['/', '\\', ':', '?', '#']) || file.starts_with('.') || !names.contains(&file) {
            return Err(format!("the icon `{src}` is not a file beside the page"));
        }
        if text(icon, "type")? != "image/png" {
            return Err(format!("the icon `{src}` is not `image/png`"));
        }
        if let Some(purpose) = icon.get("purpose")
            && !matches!(purpose.as_str(), Some("any" | "maskable"))
        {
            return Err(format!("the icon `{src}` has a purpose that is not `any` or `maskable`"));
        }
        let sizes = text(icon, "sizes")?;
        let side = sizes
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .filter(|(w, h)| w == h && *w > 0)
            .ok_or_else(|| format!("the icon `{src}` says its sizes are `{sizes}`: they are `WxW`"))?
            .0;
        let head = read_head(&folder.join(file))?;
        if png_size(&head) != Some((side, side)) {
            return Err(format!("the icon `{src}` says it is {side} by {side} and its header says {:?}", png_size(&head)));
        }
        seen.push(side);
    }
    for needed in NEEDED {
        if !seen.contains(&needed) {
            return Err(format!("there is no icon of {needed} by {needed}: a page is installable with one of 192 and one of 512"));
        }
    }
    Ok(())
}

/// The first bytes of a file: what a PNG's header is in.
fn read_head(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|file| file.take(64).read_to_end(&mut head)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(head)
}

/// Whether the start page links the manifest: a `<link rel="manifest">` whose `href` is the file.
pub(super) fn linked(html: &str) -> Result<(), String> {
    let lower = html.to_ascii_lowercase();
    for tag in lower.split('<').skip(1).filter(|t| t.starts_with("link")) {
        let tag = tag.split('>').next().unwrap_or("");
        if super::app::attribute(tag, "rel") == Some("manifest") {
            let href = super::app::attribute(tag, "href").unwrap_or("");
            return if href.strip_prefix("./").unwrap_or(href) == FILE {
                Ok(())
            } else {
                Err(format!("the page links the manifest `{href}`, and the app's is `{FILE}`"))
            };
        }
    }
    Err(format!(
        "the page does not link its manifest (<link rel=\"manifest\" href=\"{FILE}\">): the browser could not open the app in a window of its own"
    ))
}
