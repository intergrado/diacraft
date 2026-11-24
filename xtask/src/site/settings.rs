use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// The platforms a download card is for, as `data-platform` names them. Each has its own `<platform>_asset` setting.
const PLATFORMS: [&str; 3] = ["linux", "windows", "macos"];

#[derive(Debug, Clone)]
pub(super) struct Settings {
    /// The origin of the pages (`https://diacraft.dev`), without a trailing slash.
    pub(super) domain: String,
    /// The origin of the web app (`https://diacraft.app`), without a trailing slash: the pages' links to the app, and the one other site they may name besides GitHub's. A build
    /// of the site for a preview names another (`--app-url`, [`app_override`]).
    pub(super) app_url: String,
    pub(super) repository: String,
    pub(super) linux_asset: String,
    pub(super) windows_asset: String,
    pub(super) macos_asset: String,
}

impl Settings {
    /// The release asset name (or pattern) of a platform, or `None` for a platform that has no download.
    fn asset(&self, platform: &str) -> Option<&str> {
        match platform {
            "linux" => Some(&self.linux_asset),
            "windows" => Some(&self.windows_asset),
            "macos" => Some(&self.macos_asset),
            _ => None,
        }
    }
}

/// An asset name or pattern: a filename, optionally using `{tag}` or `{version}`. Empty is the draft's value.
fn valid_asset(key: &str, asset: &str) -> Result<(), String> {
    let filename = asset.replace("{tag}", "tag").replace("{version}", "version");
    if matches!(filename.as_str(), "." | "..") || filename.contains(['/', '\\', '{', '}']) || filename.contains(char::is_control) {
        return Err(format!("site.json: {key} must be a filename, optionally using {{tag}} or {{version}}"));
    }
    Ok(())
}

/// The string a key of `site.json` holds.
fn string_field<'a>(json: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    json.get(key).and_then(serde_json::Value::as_str).ok_or_else(|| format!("site.json: expected a string for {key}"))
}

/// A DNS host name: labels of letters, digits and hyphens, none empty, none over 63 bytes, none starting or ending with a hyphen, at most 253 bytes in all.
fn dns_host(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// The HTTPS origin a key of `site.json` holds, without a trailing slash: a scheme and a DNS host name, and nothing else.
fn https_origin(key: &str, raw: &str) -> Result<String, String> {
    let origin = raw.strip_suffix('/').unwrap_or(raw);
    let host = origin.strip_prefix("https://").ok_or_else(|| format!("{key} must be an HTTPS origin"))?;
    if !dns_host(host) {
        return Err(format!("{key} must contain only an HTTPS scheme and a DNS hostname"));
    }
    Ok(origin.to_owned())
}

/// The address of the app for a preview on this machine (`--app-url`, or `DIACRAFT_APP_URL`): an HTTPS origin like the setting, or `http://` with `localhost` or `127.0.0.1` and a port, and
/// nothing else, so that a preview never names a site that is not the person's own. A trailing slash is dropped.
pub(super) fn app_override(raw: &str) -> Result<String, String> {
    let origin = raw.strip_suffix('/').unwrap_or(raw);
    if let Some(local) = origin.strip_prefix("http://") {
        let (host, port) =
            local.split_once(':').ok_or_else(|| format!("`{raw}`: an http address of the app must be localhost or 127.0.0.1 with a port"))?;
        let port_ok = !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|n| n > 0);
        if !matches!(host, "localhost" | "127.0.0.1") || !port_ok {
            return Err(format!("`{raw}`: an http address of the app must be localhost or 127.0.0.1 with a port"));
        }
        return Ok(origin.to_owned());
    }
    https_origin("the app's address", origin).map_err(|e| format!("`{raw}`: {e}"))
}

pub(super) fn load(dir: &Path) -> Result<Settings, String> {
    let path = dir.join("site.json");
    let data = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&data).map_err(|e| format!("site.json: {e}"))?;
    let field = |key: &str| string_field(&json, key);
    let domain = https_origin("domain", field("domain")?).map_err(|e| format!("site.json: {e}"))?;
    let app_url = https_origin("app_url", field("app_url")?).map_err(|e| format!("site.json: {e}"))?;
    if app_url.eq_ignore_ascii_case(&domain) {
        return Err("site.json: app_url and domain must be two different origins: the web app has a site of its own".into());
    }
    let repository = field("repository")?;
    let (owner, name) = repository.split_once('/').ok_or("site.json: repository must be owner/name")?;
    if owner.is_empty() || !owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || !valid_filename(name) || name.contains('+') {
        return Err("site.json: repository must be a GitHub owner/name".into());
    }
    let mut assets = Vec::new();
    for platform in PLATFORMS {
        let key = format!("{platform}_asset");
        let asset = field(&key)?;
        valid_asset(&key, asset)?;
        assets.push(asset.to_owned());
    }
    let [linux_asset, windows_asset, macos_asset]: [String; 3] =
        assets.try_into().map_err(|_| "site.json: a platform has no asset setting".to_owned())?;
    Ok(Settings { domain, app_url, repository: repository.into(), linux_asset, windows_asset, macos_asset })
}

fn valid_filename(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

pub(super) fn release(settings: &Settings) -> Result<(), String> {
    if settings.domain.to_ascii_lowercase().ends_with(".invalid") {
        return Err("site.json: set the public domain before release".into());
    }
    if settings.app_url.to_ascii_lowercase().ends_with(".invalid") {
        return Err("site.json: set the address of the web app (app_url) before release".into());
    }
    let (owner, repo) = settings.repository.split_once('/').ok_or("site.json: repository must be owner/name")?;
    if owner.eq_ignore_ascii_case("PUBLIC-OWNER") || repo.eq_ignore_ascii_case("PUBLIC-REPO") {
        return Err("site.json: set the public repository before release".into());
    }
    for (platform, label) in [("linux", "Linux"), ("windows", "Windows"), ("macos", "macOS")] {
        if settings.asset(platform).is_none_or(str::is_empty) {
            return Err(format!("site.json: set the {label} release asset filename before release"));
        }
    }
    Ok(())
}

/// The site's source uses fixed tag/attribute prefixes. Replace only the quoted value, leaving the rest of the page byte-for-byte intact.
fn set_value(html: &mut String, prefix: &str, value: &str) -> Result<(), String> {
    let (before, after) = html.split_once(prefix).ok_or_else(|| format!("missing {prefix}"))?;
    if after.contains(prefix) {
        return Err(format!("duplicate {prefix}"));
    }
    let (old, _) = after.split_once('"').filter(|(old, _)| !old.contains(['<', '>'])).ok_or_else(|| format!("unterminated {prefix}"))?;
    let start = before.len() + prefix.len();
    html.replace_range(start..start + old.len(), value);
    Ok(())
}

fn render_page(mut source: String, settings: &Settings, url: &str) -> Result<String, String> {
    set_value(&mut source, r#"<link rel="canonical" href=""#, url)?;
    set_value(&mut source, r#"<meta property="og:url" content=""#, url)?;
    set_value(&mut source, r#"<meta property="og:image" content=""#, &format!("{}/assets/social.jpg", settings.domain))?;

    let mut output = String::with_capacity(source.len());
    let mut rest = source.as_str();
    const LINK: &str = r#"<a data-repo=""#;
    while let Some((before, link)) = rest.split_once(LINK) {
        let (suffix, attrs) = link.split_once(r#"" href=""#).ok_or("expected href immediately after data-repo")?;
        if !(suffix.is_empty() || suffix.strip_prefix('/').is_some_and(|p| p.split('/').all(valid_filename))) {
            return Err(format!("invalid data-repo suffix {suffix:?}"));
        }
        let (_, after_href) = attrs.split_once('"').filter(|(v, _)| !v.contains(['<', '>'])).ok_or("unterminated repository href")?;
        let (extra, tail) = after_href.split_once('>').ok_or("unterminated repository link")?;
        let mut extra = extra.to_owned();
        if let Some((_, after)) = extra.split_once(r#" data-platform=""#) {
            let platform = after.split_once('"').map_or("", |(name, _)| name);
            let asset = settings.asset(platform).filter(|_| suffix == "/releases/latest");
            let Some(asset) = asset else {
                return Err(format!(
                    "download cards must link to the latest release, for linux, windows or macos (found {platform:?} and {suffix:?})"
                ));
            };
            let asset = asset.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
            set_value(&mut extra, r#" data-asset=""#, &asset)?;
        }
        write!(output, r#"{before}{LINK}{suffix}" href="https://github.com/{}{suffix}"{extra}>"#, settings.repository).map_err(|e| e.to_string())?;
        rest = tail;
    }
    output.push_str(rest);
    let output = app_links(&output, settings)?;
    app_host(&output, settings)
}

/// The start of a link to the web app. The app is a site of its own (`site.json`, `app_url`), so a page that links it is written `<a data-app="" href="...">` and `--write` gives the link the
/// setting's address and a slash: the app's start page is the root of its site.
const APP_LINK: &str = r#"<a data-app="" href=""#;

/// `source` with the `href` of every `<a data-app="" href="...">` made the address of the app, and the check that no other link names the app: one that did would keep the old address
/// when the setting moves.
fn app_links(source: &str, settings: &Settings) -> Result<String, String> {
    let target = format!("{}/", settings.app_url);
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some((before, after)) = rest.split_once(APP_LINK) {
        let (_, tail) = after.split_once('"').filter(|(old, _)| !old.contains(['<', '>'])).ok_or("unterminated app href")?;
        output.push_str(before);
        output.push_str(APP_LINK);
        output.push_str(&target);
        output.push('"');
        rest = tail;
    }
    output.push_str(rest);
    let named = format!("href=\"{}", settings.app_url);
    for piece in output.split("<a ").skip(1) {
        let tag = piece.split('>').next().unwrap_or("");
        if tag.contains(&named) && !tag.starts_with(r#"data-app="" href=""#) {
            return Err(format!(
                "a link to the app ({}) must be written `<a data-app=\"\" href=...>`, so that it follows `app_url` of site.json",
                settings.app_url
            ));
        }
    }
    Ok(output)
}

/// The text of an element marked `data-app-host=""` (the address shown in the picture of a browser window) is the app's host, and its port when it has one.
fn app_host(source: &str, settings: &Settings) -> Result<String, String> {
    const MARK: &str = r#" data-app-host="">"#;
    let host = settings.app_url.split_once("://").map_or(settings.app_url.as_str(), |(_, host)| host);
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some((before, after)) = rest.split_once(MARK) {
        let (_, tail) = after.split_once('<').ok_or("unterminated data-app-host element")?;
        if !tail.starts_with('/') {
            return Err("data-app-host marks an element that holds only text".into());
        }
        output.push_str(before);
        output.push_str(MARK);
        output.push_str(host);
        output.push('<');
        rest = tail;
    }
    output.push_str(rest);
    Ok(output)
}

pub(super) fn apply(dir: &Path, settings: &Settings, write: bool) -> Result<(), String> {
    let mut public_urls = Vec::new();
    let mut outputs = Vec::new();
    for name in super::inventory(dir)? {
        let Some(stem) = name.strip_suffix(".html") else { continue };
        if !stem.split('/').all(valid_filename) {
            return Err(format!("{name}: expected a URL-safe page filename"));
        }
        let path = dir.join(&name);
        let route = format!("/{stem}");
        let route = route.strip_suffix("index").filter(|prefix| prefix.ends_with('/')).unwrap_or(&route);
        let url = format!("{}{route}", settings.domain);
        let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let page = render_page(source, settings, &url).map_err(|e| format!("{}: {e}", path.display()))?;
        outputs.push((path, page));
        if name != "404.html" {
            public_urls.push(url);
        }
    }
    let mut sitemap = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    public_urls.sort_unstable();
    for url in public_urls {
        writeln!(sitemap, "  <url><loc>{url}</loc></url>").map_err(|e| e.to_string())?;
    }
    sitemap.push_str("</urlset>\n");
    outputs.push((dir.join("sitemap.xml"), sitemap));
    outputs.push((dir.join("robots.txt"), format!("User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n", settings.domain)));
    for (path, generated) in outputs {
        match fs::read_to_string(&path) {
            Ok(actual) if actual == generated => continue,
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", path.display())),
            _ => {}
        }
        if !write {
            return Err(format!("{}: generated values are stale; run cargo xtask site --write", path.display()));
        }
        fs::write(&path, generated).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Settings, app_override, apply, load, release};
    use std::fs;
    use std::path::PathBuf;

    const PAGE: &str = r#"<html><head>
<link rel="canonical" href="old" /><meta property="og:url" content="old" />
<meta property="og:image" content="old" /></head><body>
<a data-repo="" href="old" class="link">Source</a><a data-repo="/issues" href="old">Issues</a>
<a data-app="" href="old" class="btn">Open DiaCraft in your browser</a><div class="frame-address" data-app-host="">old</div>
<a data-repo="/releases/latest" href="old" data-platform="linux" data-asset="" class="button">Linux</a>
<a data-repo="/releases/latest" href="old" data-platform="windows" data-asset="stale" class="button">Windows</a>
<a data-repo="/releases/latest" href="old" data-platform="macos" data-asset="" class="button">macOS</a>
</body></html>"#;

    struct Site(PathBuf);

    impl Site {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("diacraft-site-{name}-{}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Site {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn configured() -> Settings {
        Settings {
            domain: "https://drawing.example".into(),
            app_url: "https://app.example".into(),
            repository: "owner/drawing".into(),
            linux_asset: "linux-{version}.zip".into(),
            windows_asset: "windows-{tag}.zip".into(),
            macos_asset: "macos-{version}.zip".into(),
        }
    }

    #[test]
    fn config_accepts_draft_and_rejects_malformed_values() {
        let site = Site::new("config");
        let config = |domain: &str, repo: &str, asset: &str| {
            // A draft has a draft address for the app too; a site that is set has a real one.
            let app = if domain.ends_with(".invalid") { "https://app.invalid" } else { "https://app.example" };
            let input = format!(
                r#"{{"domain":"{domain}","app_url":"{app}","repository":"{repo}","linux_asset":"{asset}","windows_asset":"{asset}","macos_asset":"{asset}"}}"#
            );
            fs::write(site.0.join("site.json"), input).unwrap();
            load(&site.0)
        };
        let draft = config("https://diacraft.invalid", "PUBLIC-OWNER/PUBLIC-REPO", "").unwrap();
        assert!(release(&draft).is_err());
        assert!(config("https://host.example/path", "owner/drawing", "").is_err());
        assert!(config("https://host.example", "../drawing", "").is_err());
        assert!(config("https://host.example", "owner/drawing", "a/b.zip").is_err());
        assert!(config("https://host.example", "owner/drawing", "linux-{unknown}.zip").is_err());
        assert!(release(&config("https://host.example", "owner/drawing", "linux-{version}.zip").unwrap()).is_ok());
    }

    #[test]
    fn each_platform_has_its_own_asset_setting() {
        let site = Site::new("platforms");
        let json = |keys: &str| format!(r#"{{"domain":"https://host.example","app_url":"https://app.example","repository":"owner/drawing"{keys}}}"#);
        let load_keys = |keys: &str| {
            fs::write(site.0.join("site.json"), json(keys)).unwrap();
            load(&site.0)
        };
        let all = r#","linux_asset":"l-{tag}.tar.gz","windows_asset":"w-{tag}.zip","macos_asset":"m-{tag}.zip""#;
        let loaded = load_keys(all).unwrap();
        assert_eq!(
            (loaded.asset("linux"), loaded.asset("windows"), loaded.asset("macos")),
            (Some("l-{tag}.tar.gz"), Some("w-{tag}.zip"), Some("m-{tag}.zip"))
        );
        assert_eq!(loaded.asset("freebsd"), None);
        for missing in ["linux_asset", "windows_asset", "macos_asset"] {
            let without: String = all.split(',').filter(|part| !part.contains(missing)).collect::<Vec<_>>().join(",");
            let error = load_keys(&without).err().unwrap();
            assert!(error.contains(missing), "{error}");
        }
        let bad = all.replace("w-{tag}.zip", "w/{tag}.zip");
        assert!(load_keys(&bad).err().unwrap().contains("windows_asset"));
    }

    #[test]
    fn release_requires_each_setting() {
        for draft in [
            Settings { domain: "https://diacraft.invalid".into(), ..configured() },
            Settings { app_url: "https://app.invalid".into(), ..configured() },
            Settings { repository: "PUBLIC-OWNER/drawing".into(), ..configured() },
            Settings { repository: "owner/PUBLIC-REPO".into(), ..configured() },
            Settings { linux_asset: String::new(), ..configured() },
            Settings { windows_asset: String::new(), ..configured() },
            Settings { macos_asset: String::new(), ..configured() },
        ] {
            assert!(release(&draft).is_err());
        }
        assert!(release(&configured()).is_ok());
        let error = release(&Settings { windows_asset: String::new(), ..configured() }).unwrap_err();
        assert!(error.contains("Windows"), "{error}");
        let error = release(&Settings { macos_asset: String::new(), ..configured() }).unwrap_err();
        assert!(error.contains("macOS"), "{error}");
    }

    #[test]
    fn a_download_card_for_an_unknown_platform_or_another_page_is_refused() {
        let site = Site::new("cards");
        for bad in [
            r#"<a data-repo="/releases/latest" href="old" data-platform="freebsd" data-asset="">x</a>"#,
            r#"<a data-repo="/issues" href="old" data-platform="linux" data-asset="">x</a>"#,
        ] {
            let page =
                PAGE.replace(r#"<a data-repo="/releases/latest" href="old" data-platform="linux" data-asset="" class="button">Linux</a>"#, bad);
            fs::write(site.0.join("index.html"), page).unwrap();
            let error = apply(&site.0, &configured(), true).unwrap_err();
            assert!(error.contains("download cards must link to the latest release"), "{error}");
        }
    }

    #[test]
    fn write_and_check_pages_repository_and_crawlers() {
        let site = Site::new("write");
        fs::create_dir(site.0.join("guides")).unwrap();
        for file in ["index.html", "404.html", "guides/start.html"] {
            fs::write(site.0.join(file), PAGE).unwrap();
        }
        let settings = configured();
        assert!(apply(&site.0, &settings, false).is_err());
        assert_eq!(fs::read_to_string(site.0.join("index.html")).unwrap(), PAGE);
        apply(&site.0, &settings, true).unwrap();
        apply(&site.0, &settings, false).unwrap();
        let home = fs::read_to_string(site.0.join("index.html")).unwrap();
        assert!(home.contains(r#"rel="canonical" href="https://drawing.example/""#));
        let page = fs::read_to_string(site.0.join("guides/start.html")).unwrap();
        for expected in [
            r#"rel="canonical" href="https://drawing.example/guides/start""#,
            r#"property="og:url" content="https://drawing.example/guides/start""#,
            r#"property="og:image" content="https://drawing.example/assets/social.jpg""#,
            r#"data-repo="" href="https://github.com/owner/drawing" class="link""#,
            r#"data-repo="/issues" href="https://github.com/owner/drawing/issues""#,
            r#"href="https://github.com/owner/drawing/releases/latest" data-platform="linux" data-asset="linux-{version}.zip" class="button""#,
            r#"href="https://github.com/owner/drawing/releases/latest" data-platform="windows" data-asset="windows-{tag}.zip" class="button""#,
            r#"href="https://github.com/owner/drawing/releases/latest" data-platform="macos" data-asset="macos-{version}.zip" class="button""#,
        ] {
            assert!(page.contains(expected), "{expected}");
        }
        let sitemap = fs::read_to_string(site.0.join("sitemap.xml")).unwrap();
        assert!(sitemap.contains("<loc>https://drawing.example/guides/start</loc>"));
        assert!(!sitemap.contains("404"));
        assert!(fs::read_to_string(site.0.join("robots.txt")).unwrap().contains("Sitemap: https://drawing.example/sitemap.xml"));
        apply(&site.0, &Settings { linux_asset: "Linux & CLI.zip".into(), ..configured() }, true).unwrap();
        assert!(fs::read_to_string(site.0.join("index.html")).unwrap().contains(r#"data-asset="Linux &amp; CLI.zip""#));
        fs::write(site.0.join("guides/start.html"), PAGE.replace(r#"rel="canonical""#, r#"rel="other""#)).unwrap();
        assert!(apply(&site.0, &settings, true).is_err());
    }

    #[test]
    fn the_apps_address_is_a_setting_of_its_own_and_not_the_pages() {
        let site = Site::new("app-url");
        let json = |extra: &str| {
            format!(
                r#"{{"domain":"https://host.example",{extra}"repository":"owner/drawing","linux_asset":"a","windows_asset":"b","macos_asset":"c"}}"#
            )
        };
        let load_with = |extra: &str| {
            fs::write(site.0.join("site.json"), json(extra)).unwrap();
            load(&site.0)
        };
        let loaded = load_with(r#""app_url":"https://app.example/","#).unwrap();
        assert_eq!((loaded.domain.as_str(), loaded.app_url.as_str()), ("https://host.example", "https://app.example"), "a trailing slash is dropped");
        assert!(load_with("").unwrap_err().contains("app_url"), "the setting is required");
        for bad in ["http://app.example", "app.example", "https://app.example/app", "https://app.example:8100", "https://-a.example", "https://"] {
            let error = load_with(&format!(r#""app_url":"{bad}","#)).unwrap_err();
            assert!(error.contains("app_url"), "{bad}: {error}");
        }
        // Two sites: the same origin for both would put the app in the pages' project.
        let error = load_with(r#""app_url":"https://host.example","#).unwrap_err();
        assert!(error.contains("two different origins"), "{error}");
        let error = load_with(r#""app_url":"https://HOST.example/","#).unwrap_err();
        assert!(error.contains("two different origins"), "{error}");
    }

    #[test]
    fn a_preview_may_name_the_app_on_this_machine_and_nothing_else_that_is_not_https() {
        assert_eq!(app_override("http://127.0.0.1:8100/").unwrap(), "http://127.0.0.1:8100");
        assert_eq!(app_override("http://localhost:3000").unwrap(), "http://localhost:3000");
        assert_eq!(app_override("https://preview.example/").unwrap(), "https://preview.example");
        for bad in [
            "http://app.example:80",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:80/app",
            "http://127.0.0.1:8100@evil.example",
            "http://localhost.evil.example:80",
            "ftp://x.example",
            "",
            "/",
        ] {
            let error = app_override(bad).unwrap_err();
            assert!(error.contains(&format!("`{bad}`")), "{bad:?}: {error}");
        }
    }

    #[test]
    fn a_link_marked_data_app_takes_the_apps_address_and_the_address_bar_its_host() {
        let site = Site::new("data-app");
        fs::write(site.0.join("index.html"), PAGE).unwrap();
        apply(&site.0, &configured(), true).unwrap();
        let home = fs::read_to_string(site.0.join("index.html")).unwrap();
        assert!(home.contains(r#"<a data-app="" href="https://app.example/" class="btn">Open DiaCraft in your browser</a>"#), "{home}");
        assert!(home.contains(r#"<div class="frame-address" data-app-host="">app.example</div>"#), "{home}");
        apply(&site.0, &configured(), false).unwrap();
        // The setting moves, and so does everything that follows it: the check says the page is stale, and `--write` makes it current. A preview on this machine has a port in its host.
        let moved = Settings { app_url: "http://127.0.0.1:8100".into(), ..configured() };
        assert!(apply(&site.0, &moved, false).unwrap_err().contains("stale"));
        apply(&site.0, &moved, true).unwrap();
        let home = fs::read_to_string(site.0.join("index.html")).unwrap();
        assert!(home.contains(r#"<a data-app="" href="http://127.0.0.1:8100/" class="btn">"#), "{home}");
        assert!(home.contains(r#"data-app-host="">127.0.0.1:8100</div>"#), "{home}");
        // A link that names the app and is not marked would keep the old address: refused, with the way to write it.
        fs::write(
            site.0.join("index.html"),
            PAGE.replace(r#"<a data-app="" href="old" class="btn">"#, r#"<a href="https://app.example/" class="btn">"#),
        )
        .unwrap();
        let error = apply(&site.0, &configured(), true).unwrap_err();
        assert!(error.contains("data-app") && error.contains("app_url"), "{error}");
        // Malformed marks are errors, and not a panic.
        for bad in [r#"<a data-app="" href="old"#, r#"<div data-app-host="">old"#] {
            fs::write(site.0.join("index.html"), format!("{}{bad}", PAGE)).unwrap();
            assert!(apply(&site.0, &configured(), true).is_err(), "{bad}");
        }
        fs::write(site.0.join("index.html"), PAGE.replace(r#">old</div>"#, r#"><b>old</b></div>"#)).unwrap();
        assert!(apply(&site.0, &configured(), true).unwrap_err().contains("only text"));
    }
}
