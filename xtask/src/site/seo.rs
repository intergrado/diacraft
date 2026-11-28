//! What the pages give to search engines and to the cards of social sites, written by `cargo xtask site --write` and held by `cargo xtask site`.
//!
//! **Social cards.** Each page of the site has the `twitter:*` tags that go with its `og:*` ones: `twitter:card` is `summary_large_image`, and the title, the description, the picture and the
//! picture's description are the page's own `og:title`, `og:description`, `og:image` and `og:image:alt`. They are lines of the head that this module makes again from the `og:*` ones, so
//! the two sets never differ.
//!
//! **Structured data.** A page has JSON-LD (`<script type="application/ld+json" data-generated="">`, a block of data that a browser does not run and a content security policy does not see) of the kind its
//! place in the site gives it ([`Kind`]): the home page is a `SoftwareApplication` (the version, the release page, the licences and the share picture are the workspace's and `site.json`'s
//! own, so they are true on every release) and a `WebSite`; a guide is a `TechArticle`, and the guides' own index (`guides/index.html`) a `CollectionPage`; any other page that is for people
//! to read is a `WebPage`; the page for a missing address has none. A page that links a video with `<a data-video="">` (the home page's tour) has a `VideoObject` for it after
//! those, made from what the link and the elements it names say (`html::videos`, [`video_document`]).
//! Nothing is in them that the site does not say elsewhere: no rating, no review and no author. **No date of change** is in a page: the last commit that touched a page would be a commit
//! of the page, which holds the date, and the two would never settle. The dates are in `sitemap.xml`, which is another file (`lastmod`, below).
//!
//! **The text of the data.** The pages are read as XML by the checks, and a browser reads the script as raw text, so the JSON holds no `<`, `>` or `&`: each is written as the JSON escape
//! `<`, `>` or `&`, which every reader of JSON turns into the character. The keys are in alphabetical order (serde_json's map is a `BTreeMap`; a test holds that), and the
//! text is indented by lines, so that a line is one value and the export's allowlist can name the lines of the home page that carry the format words.
//!
//! **`lastmod`.** [`git_dates`] gives each page the date of the last commit that touched its file, in UTC. Where the history is not what the date needs (a shallow checkout holds the last
//! commit as the first, so every file would have its date; `git` is not there; the folder is not in a repository) it gives none, and the sitemap has no `<lastmod>`, and not a wrong one.
//! The workflows check out with a depth of one commit, so there the dates are not computed: the sitemap committed in `site/` is what is deployed, and `cargo xtask site` only holds its dates
//! to the form of a date (it never compares them with the history).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use super::html::{self, Head};
use super::settings::{self, Settings};

/// The picture every page names as its share picture, as a path of the pages' site (`site/assets/social.jpg`).
pub(super) const SOCIAL_PATH: &str = "/assets/social.jpg";

/// What that picture shows, as each page says it in `og:image:alt`, and its size in `og:image:width` and `og:image:height`: the web app's page says the same of its own copy.
pub(super) const SOCIAL_ALT: &str = "DiaCraft diagramming software, illustrated with an octopus connecting shapes.";
pub(super) const SOCIAL_SIZE: (u32, u32) = (1200, 630);

/// The name of the product, and of the site.
const NAME: &str = "DiaCraft";

/// What follows the name of a page in its `<title>`.
const TITLE_SUFFIX: &str = " · DiaCraft";

/// The context of every block of data.
const CONTEXT: &str = "https://schema.org";

/// The systems the home page says DiaCraft runs on.
const OPERATING_SYSTEMS: &str = "Windows, macOS, Linux, Web browser";

/// The licence files of the repository, by the identifier of the licence in `Cargo.toml`'s `license`: what a page names as the licence of the code. A licence that is not here is an error
/// (`licence_files`), and not a guess.
pub(super) const LICENSE_FILES: [(&str, &str); 2] = [("MIT", "LICENSE-MIT"), ("Apache-2.0", "LICENSE-APACHE")];

/// The `twitter:*` tags this module makes: a tag with another name (`twitter:site`, written by hand) is the page's own and stays.
const TWITTER_TAGS: [&str; 5] = ["twitter:card", "twitter:title", "twitter:description", "twitter:image", "twitter:image:alt"];

/// The line that opens a block of data of a page. The generator owns every block that opens this way (`data-generated`, as `data-repo` and `data-app` mark what it makes), and leaves a block
/// that a person wrote, with no mark, as it is.
const JSON_LD_OPEN: &str = r#"<script type="application/ld+json" data-generated="">"#;

/// What a page is in the site, which says what its structured data is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// The home page.
    Home,
    /// A guide (`guides/`).
    Guide,
    /// The index of the guides (`guides/index.html`): a page that lists them.
    Index,
    /// Any other page that is for people to read.
    Page,
    /// The page for an address that is not there: it has no structured data.
    NotFound,
}

/// The [`Kind`] of the page file `name` (`index.html`, `guides/getting-started.html`).
pub(super) fn kind(name: &str) -> Kind {
    if name == "index.html" {
        Kind::Home
    } else if name == "404.html" {
        Kind::NotFound
    } else if name == "guides/index.html" {
        Kind::Index
    } else if name.starts_with("guides/") {
        Kind::Guide
    } else {
        Kind::Page
    }
}

/// The licence files of the repository for the licence expression of `Cargo.toml` (`MIT OR Apache-2.0`): the file of each licence in it. An expression with another operator, or a
/// licence that [`LICENSE_FILES`] does not list, is an error.
pub(super) fn licence_files(expression: &str) -> Result<Vec<&'static str>, String> {
    let mut files = Vec::new();
    for id in expression.split(" OR ").map(str::trim) {
        let (_, file) = LICENSE_FILES
            .iter()
            .find(|(known, _)| *known == id)
            .ok_or_else(|| format!("the licence `{id}` of Cargo.toml has no file in the list of xtask/src/site/seo.rs (LICENSE_FILES): add it with the file that holds its text"))?;
        files.push(*file);
    }
    Ok(files)
}

/// The address of the page of a file of the repository, on GitHub: `https://github.com/<owner>/<name>/blob/HEAD/<path>`.
fn blob(settings: &Settings, path: &str) -> String {
    format!("https://github.com/{}/blob/HEAD/{path}", settings.repository)
}

/// `text` as the value of an attribute in double quotes, on one line: the markup characters are their named references, and a line break, a carriage return and a tab are their numeric ones
/// (`&#10;`, `&#13;`, `&#9;`). A value that came from a reference to one of them (`og:title` content `A&#10;B`) is written as it was, and not as a character that ends the line of the tag, which
/// `without_generated` could not take away whole.
fn attribute(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

/// The `twitter:*` lines of a page's head, from its `og:*` values.
fn twitter(head: &Head) -> String {
    let mut lines = String::new();
    let mut tag = |name: &str, content: &str| {
        // Writing to a `String` cannot fail.
        let _ = writeln!(lines, "  <meta name=\"{name}\" content=\"{}\" />", attribute(content));
    };
    tag("twitter:card", "summary_large_image");
    tag("twitter:title", &head.og_title);
    tag("twitter:description", &head.og_description);
    tag("twitter:image", &head.og_image);
    if let Some(alt) = &head.og_image_alt {
        tag("twitter:image:alt", alt);
    }
    lines
}

/// What the page's name is when it is called by its `<title>`: the title without ` · DiaCraft` at its end.
fn name_of(title: &str) -> &str {
    title.strip_suffix(TITLE_SUFFIX).filter(|name| !name.is_empty()).unwrap_or(title)
}

/// The site, as a part of the data of a page.
fn site(settings: &Settings) -> Value {
    json!({"@type": "WebSite", "name": NAME, "url": format!("{}/", settings.domain)})
}

/// The blocks of data of a page of the kind `kind`, at the address `url`, with the head `head`.
pub(super) fn documents(kind: Kind, head: &Head, settings: &Settings, url: &str) -> Vec<Value> {
    let image = format!("{}{SOCIAL_PATH}", settings.domain);
    match kind {
        Kind::Home => {
            let mut application = json!({
                "@context": CONTEXT,
                "@type": "SoftwareApplication",
                "name": NAME,
                "url": url,
                "description": head.description,
                "applicationCategory": "DesignApplication",
                "operatingSystem": OPERATING_SYSTEMS,
                "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
                "softwareVersion": settings.version,
                "downloadUrl": settings::direct_address(settings, ""),
                "image": image,
                "sameAs": [format!("https://github.com/{}", settings.repository)],
            });
            let licences: Vec<String> = settings.licences.iter().map(|file| blob(settings, file)).collect();
            if let (false, Some(object)) = (licences.is_empty(), application.as_object_mut()) {
                object.insert("license".to_owned(), json!(licences));
            }
            let mut website = site(settings);
            if let Some(object) = website.as_object_mut() {
                object.insert("@context".to_owned(), json!(CONTEXT));
            }
            vec![application, website]
        }
        Kind::Guide => vec![json!({
            "@context": CONTEXT,
            "@type": "TechArticle",
            "headline": name_of(&head.title),
            "description": head.description,
            "url": url,
            "image": image,
            "inLanguage": "en",
            "isPartOf": site(settings),
        })],
        Kind::Page | Kind::Index => vec![json!({
            "@context": CONTEXT,
            "@type": if kind == Kind::Index { "CollectionPage" } else { "WebPage" },
            "name": name_of(&head.title),
            "description": head.description,
            "url": url,
            "inLanguage": "en",
            "isPartOf": site(settings),
        })],
        Kind::NotFound => Vec::new(),
    }
}

/// The start of the address of the player of a video on YouTube, which a page may give a search engine as the video's `embedUrl`: the player, and not the page of the video.
const VIDEO_PLAYER: &str = "https://www.youtube.com/embed/";

/// The structured data of a video a page links: a `VideoObject` made from what the page says of it (`html::videos`). Its picture is the page's own copy, at the pages' address; its
/// player is YouTube's for its id; its `url` is the address the page links. It has no `contentUrl`, which is the address of the video's own bytes (the file), and YouTube gives none.
pub(super) fn video_document(video: &html::Video, settings: &Settings) -> Value {
    json!({
        "@context": CONTEXT,
        "@type": "VideoObject",
        "name": video.name,
        "description": video.description,
        "thumbnailUrl": format!("{}{}", settings.domain, video.thumbnail),
        "uploadDate": video.upload_date,
        "duration": video.duration,
        "embedUrl": format!("{VIDEO_PLAYER}{}", video.id),
        "url": video.address,
    })
}

/// `value` as the lines of a block of data in the head: the opening line, the JSON indented, and the closing line. The JSON has no `<`, `>` or `&` (each is its JSON escape), so that the page is
/// well-formed XML and no text can end the script.
pub(super) fn script(value: &Value) -> Result<String, String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("structured data: {e}"))?;
    let text = text.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026");
    let mut block = format!("  {JSON_LD_OPEN}\n");
    for line in text.lines() {
        // Writing to a `String` cannot fail.
        let _ = writeln!(block, "    {line}");
    }
    block.push_str("  </script>\n");
    Ok(block)
}

/// `source` without the lines this module writes: the `twitter:*` tags and the blocks of data. A block that is not closed is an error.
fn without_generated(source: &str) -> Result<String, String> {
    let mut out = String::with_capacity(source.len());
    let mut in_block = false;
    for line in source.split_inclusive('\n') {
        let text = line.trim();
        if in_block {
            in_block = text != "</script>";
        } else if text == JSON_LD_OPEN {
            in_block = true;
        } else if !TWITTER_TAGS.iter().any(|name| text.starts_with(&format!("<meta name=\"{name}\""))) {
            out.push_str(line);
        }
    }
    if in_block {
        return Err("a block of structured data that is not closed".into());
    }
    Ok(out)
}

/// `source` (a page whose `og:*` tags and canonical address are already made) with its `twitter:*` tags and its structured data made: the tags after the last `og:*` tag of the head, and the
/// data before `</head>`, which must start a line. What was there is replaced, so that the result is the same however often it is made.
pub(super) fn render(source: &str, settings: &Settings, name: &str, url: &str) -> Result<String, String> {
    let head = html::head(source)?;
    let plain = without_generated(source)?;
    let lines: Vec<&str> = plain.split_inclusive('\n').collect();
    let end = lines
        .iter()
        .position(|line| line.trim_start().starts_with("</head>"))
        .ok_or("put </head> at the start of a line, so that the structured data can go before it")?;
    let last_og = lines
        .get(..end)
        .and_then(|before| before.iter().rposition(|line| line.trim_start().starts_with("<meta property=\"og:")))
        .ok_or("the head has no og: tag to put the twitter: tags after")?;
    // The videos the page links follow the page's own data, each a block of its own. The page for a missing address has no data, so it may link none.
    let videos = html::videos(source)?;
    if kind(name) == Kind::NotFound && !videos.is_empty() {
        return Err("the page for a missing address has no structured data, so it links no video marked data-video".into());
    }
    let mut data = String::new();
    for document in documents(kind(name), &head, settings, url).into_iter().chain(videos.iter().map(|video| video_document(video, settings))) {
        data.push_str(&script(&document)?);
    }
    let mut out = String::with_capacity(plain.len() + 2048);
    for (n, line) in lines.iter().enumerate() {
        if n == end {
            out.push_str(&data);
        }
        out.push_str(line);
        if n == last_og {
            out.push_str(&twitter(&head));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------------ dates

/// Whether `text` is a date of the form `YYYY-MM-DD` that exists (a day of that month, in that year).
pub(super) fn valid_date(text: &str) -> bool {
    let shape = text.len() == 10 && text.bytes().enumerate().all(|(n, b)| if matches!(n, 4 | 7) { b == b'-' } else { b.is_ascii_digit() });
    shape && text.parse::<jiff::civil::Date>().is_ok_and(|date| date.to_string() == text)
}

/// Whether `text` is the length of a video as ISO 8601 writes a duration and search engines read it (`PT6M53S`): `PT`, then hours, minutes and seconds, each a number of one to four digits and its
/// letter (`H`, `M`, `S`), in that order, each at most once, and at least one of them. A duration of days, months or years, a fraction and a sign are not a video's length here.
pub(super) fn valid_duration(text: &str) -> bool {
    let Some(mut rest) = text.strip_prefix("PT") else { return false };
    let mut parts = 0;
    for unit in ['H', 'M', 'S'] {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=4).contains(&digits)
            && let Some(after) = rest.get(digits..).and_then(|after| after.strip_prefix(unit))
        {
            rest = after;
            parts += 1;
        }
    }
    parts > 0 && rest.is_empty()
}

/// Runs `git` in `root` with the arguments, and gives what it printed. Any failure (no `git`, not a repository, a non-zero exit) is `None`.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        // The dates are the commits' own, in UTC: not the time zone of the machine that runs this.
        .env("TZ", "UTC")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// What [`git_dates`] found: the date of the last commit that touched each of the files that has one, or why no date was given.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Dates {
    Found(BTreeMap<String, String>),
    /// No dates, and the reason in words.
    Unavailable(String),
}

/// The date (`YYYY-MM-DD`, UTC) of the last commit that touched each of `files` (paths from `root`, the root of the repository), by `git log`. A file that no commit has touched (it is new, or not
/// tracked) is not in the map. **No dates at all** when the history cannot be relied on: `git` is missing, `root` is not in a repository, or the repository is shallow, where the first commit of
/// what was fetched holds every file and gives each its date.
pub(super) fn git_dates(root: &Path, files: &[String]) -> Dates {
    let Some(shallow) = git(root, &["rev-parse", "--is-shallow-repository"]) else {
        return Dates::Unavailable("git has no history for this folder".into());
    };
    if shallow != "false" {
        return Dates::Unavailable("this checkout is shallow: it holds the last commit only, which would give every page its date".into());
    }
    let mut found = BTreeMap::new();
    for file in files {
        let date = git(root, &["log", "-1", "--format=%cd", "--date=format-local:%Y-%m-%d", "--", file]);
        if let Some(date) = date.filter(|date| valid_date(date)) {
            found.insert(file.clone(), date);
        }
    }
    Dates::Found(found)
}

// ------------------------------------------------------------------------------------------------------------------------ the web app

/// The name of the share picture in the web app's folder: a copy of the pages' (`site/assets/social.jpg`), so that the app's page names a picture that its own site serves (the deploy check asks
/// each site for every picture its pages name, and a picture of the other site is not one it serves).
pub(super) const APP_SOCIAL: &str = "social.jpg";

/// What the app needs of a browser, in words, from what the page about the app says of the browsers (`site/browser.html`, "Which browsers work").
const BROWSER_REQUIREMENTS: &str = "Requires a recent desktop browser with WebAssembly and WebGPU or WebGL 2.";

/// The place of the `>` that ends the start tag whose name ends just before `from` in `html`, with the quotes of the values in mind: a `>` between the quotes of a value (`content="A > B"`,
/// `content='A > B'`) is a part of the value. A quote opens a value only straight after `=` (white space may come between), as an HTML reader takes it; anywhere else it is a letter. `None` when the tag
/// is not closed: no `>` follows, or a quote is not closed.
fn tag_end(html: &str, from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    // The last byte that was not white space was `=`.
    let mut after_equals = false;
    for (n, byte) in html.as_bytes().get(from..)?.iter().enumerate() {
        if let Some(open) = quote {
            if *byte == open {
                quote = None;
            }
        } else if *byte == b'>' {
            return from.checked_add(n);
        } else if matches!(byte, b'"' | b'\'') && after_equals {
            quote = Some(*byte);
            after_equals = false;
        } else if !byte.is_ascii_whitespace() {
            after_equals = *byte == b'=';
        }
    }
    None
}

/// The start tags `<name ...>` of `html`, each from its `<` to its `>` (`tag_end`). The web app's page is HTML as Trunk writes it, with tags that are not closed, so it is read as text and not as XML.
///
/// The page is read in one pass: a tag is read to its end and the next is looked for after it, so what a tag holds in its quotes (`<meta` in a value) is no tag, and a page that is all open tags is read
/// in the time its size asks, and not its size times its tags. A tag that is not closed leaves nothing after it to read.
fn tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{name}");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find(&open)) {
        let start = from + at;
        let after = start + open.len();
        let named = lower.get(after..).and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_whitespace() || c == '>' || c == '/');
        if !named {
            from = after;
            continue;
        }
        let Some(tag) = tag_end(html, after).and_then(|end| html.get(start..=end)) else { break };
        found.push(tag);
        from = start + tag.len();
    }
    found
}

/// The `content` of the `<meta>` with `key` equal to `value` (`name="description"`), as the text it means.
fn meta<'a>(html: &'a str, key: &str, value: &str) -> Option<&'a str> {
    tags(html, "meta").into_iter().find(|tag| super::app::attribute(tag, key) == Some(value)).and_then(|tag| super::app::attribute(tag, "content"))
}

/// The longest reference [`unescape`] reads, from after the `&` to before the `;`, in bytes: `#x10FFFF` is eight, and zeros before a number are allowed up to this length. A reference that the
/// `;` does not close within it is not read as one.
const MAX_REFERENCE: usize = 32;

/// The character of the code point `value`, when it is one a page may write as a reference: not 0, not a surrogate (U+D800 to U+DFFF), not past U+10FFFF, and not one of the control characters
/// U+0080 to U+009F, which a browser reads as other characters (the Windows-1252 ones) and so would not mean what this reads.
pub(super) fn code_point(value: u32) -> Option<char> {
    if value == 0 || (0x80..=0x9f).contains(&value) { None } else { char::from_u32(value) }
}

/// The character that the number of a reference stands for (`#38`, `#x26`, `#X26`: the text between the `&` and the `;`), if it is a number of digits, with no sign and no space, that is a character
/// ([`code_point`]).
fn numeric(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let (digits, radix) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (digits, 10),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0u32;
    for digit in digits.chars() {
        value = value.checked_mul(radix)?.checked_add(digit.to_digit(radix)?)?;
    }
    code_point(value)
}

/// The character of the named reference `name` (the text between the `&` and the `;`): the five that an attribute needs.
fn named(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "amp" => Some('&'),
        _ => None,
    }
}

/// `text` as the characters that the references of an attribute stand for: the five named ones (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`) and the numeric ones, in decimal (`&#38;`) and in
/// hexadecimal (`&#x26;`, `&#X26;`), with zeros before the number allowed. A reference is read once: what it stands for is not read as another (`&#38;lt;` is the four characters `&lt;`). Another named
/// reference, and a `&` that starts none, is text as it is.
///
/// A `&#` that is not a reference of a character is an error: a number that is no character ([`code_point`]: 0, a surrogate, past U+10FFFF, U+0080 to U+009F, or past what 32 bits hold), one with no digit or
/// with something else than digits, and one that no `;` closes within [`MAX_REFERENCE`] bytes. Each `&` is looked at in at most [`MAX_REFERENCE`] bytes after it, so that the work is that of the size of `text`.
pub(super) fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        // The text of a reference is what comes before a `;` in the first bytes after the `&`, and `used` is how far the reference goes (its `;` too).
        let closed = after.bytes().take(MAX_REFERENCE + 1).position(|byte| byte == b';').and_then(|end| Some((after.get(..end)?, end + 1)));
        let decoded = match closed {
            Some((body, used)) if body.starts_with('#') => {
                Some((numeric(body).ok_or_else(|| format!("the character reference `&{body};` is not a character"))?, used))
            }
            Some((body, used)) => named(body).map(|c| (c, used)),
            None if after.starts_with('#') => {
                let shown: String = after.chars().take(16).collect();
                return Err(format!("the character reference `&{shown}` is not closed by a `;` within {MAX_REFERENCE} bytes"));
            }
            None => None,
        };
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = after.get(used..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// The words of the page's `<title>`, as the page writes them, with no white space around them (the references are not read: [`unescape`]).
fn title_of(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = start + lower.get(start..)?.find("</title>")?;
    html.get(start..end).map(str::trim)
}

/// The text of `raw`, a title or the value of a tag as the page writes it: its references are the characters they stand for, and it is not empty. `what` names it in the error of a reference that is not a
/// character, and `missing` is the error of a page that has none or an empty one.
fn text_of(raw: Option<&str>, what: &str, missing: &str) -> Result<String, String> {
    let text = unescape(raw.ok_or(missing)?).map_err(|e| format!("the page's {what}: {e}"))?;
    if text.is_empty() { Err(missing.to_owned()) } else { Ok(text) }
}

/// The address of the web app's start page: its origin and a slash.
fn app_address(settings: &Settings) -> String {
    format!("{}/", settings.app_url)
}

/// The structured data of the web app's page: a `WebApplication`, free, with what it needs of a browser.
pub(super) fn app_document(description: &str, settings: &Settings) -> Value {
    json!({
        "@context": CONTEXT,
        "@type": "WebApplication",
        "name": NAME,
        "url": app_address(settings),
        "description": description,
        "applicationCategory": "DesignApplication",
        "operatingSystem": "Any",
        "browserRequirements": BROWSER_REQUIREMENTS,
        "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
        "image": format!("{}/{APP_SOCIAL}", settings.app_url),
        "sameAs": [format!("https://github.com/{}", settings.repository)],
    })
}

/// `html`, the web app's page, with the tags a search engine and a social card read, made from the page's own title and description and from the settings: the canonical address (the app's,
/// `app_url`), `og:*`, `twitter:*` and a `WebApplication` block, put before `</head>`. A page that has any of them already is an error: Trunk's page is made fresh each time, and what is
/// there would be said twice.
pub(super) fn app_page(html: &str, settings: &Settings) -> Result<String, String> {
    let title = text_of(title_of(html), "title", "the page has no title")?;
    let description = text_of(meta(html, "name", "description"), "description", "the page has no <meta name=\"description\">")?;
    let lower = html.to_ascii_lowercase();
    for held in ["rel=\"canonical\"", "property=\"og:", "name=\"twitter:", "application/ld+json"] {
        if lower.contains(held) {
            return Err(format!("the page has {held} already: the build makes it"));
        }
    }
    let address = app_address(settings);
    let image = format!("{}/{APP_SOCIAL}", settings.app_url);
    let mut block = String::new();
    let mut line = |text: String| {
        block.push_str(&text);
        block.push('\n');
    };
    let og = |property: &str, content: &str| format!("<meta property=\"{property}\" content=\"{}\">", attribute(content));
    let card = |name: &str, content: &str| format!("<meta name=\"{name}\" content=\"{}\">", attribute(content));
    line(format!("<link rel=\"canonical\" href=\"{}\">", attribute(&address)));
    line(og("og:type", "website"));
    line(og("og:site_name", NAME));
    line(og("og:title", &title));
    line(og("og:description", &description));
    line(og("og:url", &address));
    line(og("og:image", &image));
    line(og("og:image:alt", SOCIAL_ALT));
    line(og("og:image:width", &SOCIAL_SIZE.0.to_string()));
    line(og("og:image:height", &SOCIAL_SIZE.1.to_string()));
    line(card("twitter:card", "summary_large_image"));
    line(card("twitter:title", &title));
    line(card("twitter:description", &description));
    line(card("twitter:image", &image));
    line(card("twitter:image:alt", SOCIAL_ALT));
    block.push_str(&script(&app_document(&description, settings))?);
    let end = lower.find("</head>").ok_or("the page has no </head>")?;
    let mut out = String::with_capacity(html.len() + block.len());
    out.push_str(html.get(..end).ok_or("the page ends inside a tag")?);
    out.push_str(&block);
    out.push_str(html.get(end..).ok_or("the page ends inside a tag")?);
    Ok(out)
}

/// The app's `robots.txt`: every crawler may read every file, and the sitemap is named.
pub(super) fn app_robots(settings: &Settings) -> String {
    format!("User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n", settings.app_url)
}

/// The app's `sitemap.xml`: its one address. It has no `<lastmod>`: the app is built, not committed, and a date of its build would change with every build.
pub(super) fn app_sitemap(settings: &Settings) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n  <url><loc>{}</loc></url>\n</urlset>\n",
        app_address(settings)
    )
}

/// What the check of the web app's folder holds of its search and social data: the page has a title, a description and a canonical address that is the app's start page (an HTTPS address of
/// a host, a slash), `og:*` and `twitter:*` tags that say what the page says and name the share picture the folder has, a `WebApplication` block that is free and says what it needs of a
/// browser, and the folder has a `robots.txt` that lets every crawler in and names the sitemap, and a `sitemap.xml` of that one address. `names` are the files of the folder.
pub(super) fn check_app(html: &str, robots: &str, sitemap: &str, names: &[&str]) -> Result<(), String> {
    let title = text_of(title_of(html), "title", "no <title>")?;
    let description = text_of(meta(html, "name", "description"), "description", "no <meta name=\"description\">")?;
    let canonical = tags(html, "link")
        .into_iter()
        .find(|tag| super::app::attribute(tag, "rel") == Some("canonical"))
        .and_then(|tag| super::app::attribute(tag, "href"))
        .ok_or("no canonical address (<link rel=\"canonical\">)")?;
    let origin = canonical
        .strip_suffix('/')
        .filter(|origin| origin.starts_with("https://") && !origin.contains(['?', '#']) && origin.len() > "https://".len());
    let origin = origin
        .filter(|origin| !origin.get("https://".len()..).is_some_and(|host| host.contains('/')))
        .ok_or_else(|| format!("the canonical address `{canonical}` is not the app's start page: an HTTPS address with a host and a slash"))?;
    let image = format!("{origin}/{APP_SOCIAL}");
    if !names.contains(&APP_SOCIAL) {
        return Err(format!("the page names the share picture {image}, and the folder has no {APP_SOCIAL}"));
    }
    for (property, want) in [
        ("og:type", "website"),
        ("og:site_name", NAME),
        ("og:title", title.as_str()),
        ("og:description", description.as_str()),
        ("og:url", canonical),
        ("og:image", image.as_str()),
        ("og:image:alt", SOCIAL_ALT),
    ] {
        let found = meta(html, "property", property).map(unescape).transpose().map_err(|e| format!("{property}: {e}"))?;
        if found.as_deref() != Some(want) {
            return Err(format!("{property} is {found:?} and must be `{want}`"));
        }
    }
    for (name, want) in [
        ("twitter:card", "summary_large_image"),
        ("twitter:title", title.as_str()),
        ("twitter:description", description.as_str()),
        ("twitter:image", image.as_str()),
        ("twitter:image:alt", SOCIAL_ALT),
    ] {
        let found = meta(html, "name", name).map(unescape).transpose().map_err(|e| format!("{name}: {e}"))?;
        if found.as_deref() != Some(want) {
            return Err(format!("{name} is {found:?} and must be `{want}`"));
        }
    }
    let lower = html.to_ascii_lowercase();
    let open = lower.find(JSON_LD_OPEN).ok_or("no WebApplication data (<script type=\"application/ld+json\" data-generated=\"\">)")?;
    let start = open + JSON_LD_OPEN.len();
    let end = start + lower.get(start..).and_then(|rest| rest.find("</script>")).ok_or("a block of data that is not closed")?;
    if lower.matches("application/ld+json").count() != 1 {
        return Err("more than one block of data: the page is one application".into());
    }
    let data: Value = serde_json::from_str(html.get(start..end).unwrap_or("")).map_err(|e| format!("the data is not JSON: {e}"))?;
    let want = app_document_shape(&data)?;
    if want != (canonical, true, true) {
        return Err(format!("the WebApplication data must be at {canonical}, free, and say what it needs of a browser"));
    }
    let expected_robots = format!("User-agent: *\nAllow: /\n\nSitemap: {origin}/sitemap.xml\n");
    if robots != expected_robots {
        return Err(format!("robots.txt must let every crawler in and name the sitemap: {expected_robots:?}"));
    }
    if !sitemap.contains(&format!("<loc>{canonical}</loc>")) || sitemap.matches("<url>").count() != 1 {
        return Err(format!("sitemap.xml must hold the one address {canonical}"));
    }
    Ok(())
}

/// What the app's data says: its address, whether it is free (an offer of price 0) and whether it says what it needs of a browser. A data block that is not a `WebApplication` of the
/// schema.org context is an error.
fn app_document_shape(data: &Value) -> Result<(&str, bool, bool), String> {
    if data.get("@context").and_then(Value::as_str) != Some(CONTEXT) || data.get("@type").and_then(Value::as_str) != Some("WebApplication") {
        return Err("the data must be a WebApplication of https://schema.org".into());
    }
    let url = data.get("url").and_then(Value::as_str).unwrap_or("");
    let free = data.pointer("/offers/price").and_then(Value::as_str) == Some("0");
    let browsers = data.get("browserRequirements").and_then(Value::as_str).is_some_and(|text| !text.trim().is_empty());
    Ok((url, free, browsers))
}
