use super::File;
use serde_json::Value;

const SITEMAP_NS: &str = "http://www.sitemaps.org/schemas/sitemap/0.9";

/// The origins the pages may name: the site's own, the web app's (a site of its own) and GitHub's, over HTTPS. A preview of the build names the app on this machine, over
/// plain HTTP (`settings::app_override`): that is the one origin that is not HTTPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Origins {
    /// `https://host` of the pages, with no trailing slash.
    pub(super) domain: String,
    /// `https://host` of the app (or `http://127.0.0.1:port` for a preview), with no trailing slash.
    pub(super) app: String,
}

impl Origins {
    pub(super) fn new(domain: &str, app: &str) -> Self {
        Origins { domain: domain.to_owned(), app: app.to_owned() }
    }

    /// The host (and port) of the pages.
    pub(super) fn domain_host(&self) -> &str {
        self.domain.split_once("://").map_or(self.domain.as_str(), |(_, host)| host)
    }

    /// Whether a link with this scheme (none for a protocol-relative one) to `host` is one the pages may have.
    fn allows(&self, scheme: Option<&str>, host: &str) -> bool {
        let https = scheme.is_none_or(|scheme| scheme.eq_ignore_ascii_case("https"));
        if https && ["github.com", "api.github.com", self.domain_host()].iter().any(|allowed| host.eq_ignore_ascii_case(allowed)) {
            return true;
        }
        // The app's origin as it is written: the scheme it has (https, or http for a preview) and its host.
        let (app_scheme, app_host) = self.app.split_once("://").unwrap_or(("https", self.app.as_str()));
        scheme.is_none_or(|scheme| scheme.eq_ignore_ascii_case(app_scheme)) && host.eq_ignore_ascii_case(app_host)
    }
}

/// Finds literal web origins in public prose, scripts, styles and deployment configuration, including protocol-relative URLs.
pub(super) fn hosts(text: &str, origins: &Origins, name: &str) -> Result<(), String> {
    for (i, _) in text.match_indices("//") {
        let before = &text[..i];
        let after = &text[i + 2..];
        let host = after.split(|c: char| c.is_whitespace() || "/?#\"'\x60<>(){}[],;".contains(c)).next().unwrap_or("");
        let scheme =
            before.strip_suffix(':').map(|s| s.rsplit(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '+' | '-' | '.')).next().unwrap_or(""));
        if scheme.is_none() {
            let literal = before.ends_with(['"', '\'', '\x60', '(', '=']);
            let prose = (before.is_empty() || before.ends_with(char::is_whitespace)) && host.contains('.');
            // A JavaScript line comment also starts with two slashes.
            if (!literal || after.starts_with(char::is_whitespace)) && !prose {
                continue;
            }
        }
        let namespace = name == "sitemap.xml"
            && before.strip_suffix("http:").is_some_and(|s| s.ends_with("xmlns=\"") || s.ends_with("xmlns='"))
            && after.strip_prefix(SITEMAP_NS.trim_start_matches("http://")).is_some_and(|s| s.starts_with(['"', '\'']));
        // The context of structured data (`seo.rs`) is a name and no address that is loaded: the one line the generator writes, and no other use of the host.
        let schema = before.ends_with("\"@context\": \"https:")
            && host == "schema.org"
            && after.strip_prefix("schema.org").is_some_and(|s| s.starts_with('"'));
        if namespace || schema {
            continue;
        }
        if !origins.allows(scheme, host) {
            return Err(format!("unapproved origin near //{host}"));
        }
    }
    Ok(())
}

pub(super) fn percent_decode(input: &str) -> Result<String, String> {
    let mut bytes = input.bytes();
    let mut out = Vec::with_capacity(input.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut digit = || bytes.next().and_then(|b| char::from(b).to_digit(16)).ok_or("invalid percent escape");
            out.push((digit()? * 16 + digit()?) as u8);
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).map_err(|e| format!("invalid UTF-8 in URL: {e}"))
}

/// None means a permitted external URL. Local paths are resolved against their referring file, never the process directory.
pub(super) fn resolve(from: &str, url: &str, origins: &Origins) -> Result<Option<(String, String)>, String> {
    if url.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return Err("empty URL or invalid URL character".into());
    }
    let (url, fragment) = url.split_once('#').unwrap_or((url, ""));
    let url = url.split('?').next().unwrap_or("");
    let mut path = url;
    if let Some((scheme, rest)) = url.split_once("://").filter(|(s, _)| !s.contains('/')) {
        let host = rest.split('/').next().unwrap_or("");
        if !origins.allows(Some(scheme), host) {
            return Err("unapproved URL origin".into());
        }
        // The pages' own address is a local path; the app's and GitHub's are permitted addresses of other sites.
        if !(scheme.eq_ignore_ascii_case("https") && host.eq_ignore_ascii_case(origins.domain_host())) {
            return Ok(None);
        }
        path = rest.get(host.len()..).filter(|s| !s.is_empty()).unwrap_or("/");
    } else if url.starts_with("//") || url.split('/').next().is_some_and(|first| first.contains(':')) {
        return Err("expected a local URL or an allowed HTTPS origin".into());
    }
    let fragment = percent_decode(fragment)?;
    if path.is_empty() {
        return Ok(Some((from.to_owned(), fragment)));
    }
    let path = percent_decode(path)?;
    if path.contains('\\') || path.chars().any(char::is_control) {
        return Err("invalid character in URL path".into());
    }
    let mut segments = Vec::new();
    if !path.starts_with('/')
        && let Some((dir, _)) = from.rsplit_once('/')
    {
        segments.extend(dir.split('/'));
    }
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop().ok_or("URL escapes the site root")?;
            }
            part => segments.push(part),
        }
    }
    if path.ends_with('/') || segments.is_empty() {
        segments.push("index.html");
    }
    Ok(Some((segments.join("/"), fragment)))
}

/// Manifest images contribute to page weight. Navigation (e.g. the application start URL) only needs to resolve.
pub(super) fn json(value: &Value, origins: &Origins, manifest: bool, file: &mut File) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                json(item, origins, manifest, file)?;
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if manifest && matches!(key.as_str(), "src" | "start_url" | "url" | "scope" | "id") {
                    let url = value.as_str().ok_or_else(|| format!("expected a URL for {key}"))?;
                    if key == "src" {
                        file.assets.push(url.to_owned());
                    } else {
                        file.links.push(url.to_owned());
                    }
                }
                json(value, origins, manifest, file)?;
            }
        }
        Value::String(text) => {
            super::brands(text, &[])?;
            hosts(text, origins, "")?;
        }
        _ => {}
    }
    Ok(())
}

/// The stylesheet may use ordinary URL tokens or quoted imports. Reject escaped URL spellings that this static checker cannot resolve.
pub(super) fn css(source: &str) -> Result<Vec<String>, String> {
    let mut rest = source;
    let mut urls = Vec::new();
    while let Some(c) = rest.chars().next() {
        if let Some(comment) = rest.strip_prefix("/*") {
            rest = comment.split_once("*/").ok_or("unterminated CSS comment")?.1;
        } else if matches!(c, '"' | '\'') {
            rest = quoted(rest)?.1;
        } else if c.is_ascii_alphanumeric() || matches!(c, '@' | '-' | '_') {
            let len = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'-' | b'_')).count();
            let (word, after) = rest.split_at(len);
            rest = after;
            if word.eq_ignore_ascii_case("url")
                && let Some(arg) = rest.trim_start().strip_prefix('(')
            {
                let arg = arg.trim_start();
                let (url, tail) = if arg.starts_with(['"', '\'']) {
                    let (url, tail) = quoted(arg)?;
                    (url, tail.trim_start().strip_prefix(')').ok_or("unterminated CSS url()")?)
                } else {
                    arg.split_once(')').ok_or("unterminated CSS url()")?
                };
                urls.push(url.trim().to_owned());
                rest = tail;
            } else if word.eq_ignore_ascii_case("@import") && rest.trim_start().starts_with(['"', '\'']) {
                let (url, tail) = quoted(rest.trim_start())?;
                urls.push(url.to_owned());
                rest = tail;
            }
        } else {
            rest = &rest[c.len_utf8()..];
        }
    }
    Ok(urls)
}

fn quoted(text: &str) -> Result<(&str, &str), String> {
    let delimiter = text.chars().next().ok_or("missing CSS quote")?;
    let rest = text.strip_prefix(delimiter).ok_or("missing CSS quote")?;
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delimiter {
            return Ok((&rest[..i], &rest[i + c.len_utf8()..]));
        }
    }
    Err("unterminated CSS string".into())
}
