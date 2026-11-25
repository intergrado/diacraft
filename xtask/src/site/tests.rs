use super::check;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const DOMAIN: &str = "https://example.invalid";
const HEADERS: &str = "/*\n  Content-Security-Policy: default-src 'self'; script-src 'self'; connect-src https://api.github.com; img-src 'self'; style-src 'self'; frame-ancestors 'none'\n  X-Content-Type-Options: nosniff\n  Referrer-Policy: no-referrer\n  Permissions-Policy: camera=(), microphone=(), geolocation=()\n";
const HTML: &str = r##"<!DOCTYPE html><html lang="en"><head><title>Drawing</title><meta name="description" content="A drawing guide." />
<link rel="canonical" href="https://example.invalid/" /><meta property="og:url" content="https://example.invalid/" />
<meta property="og:image" content="https://example.invalid/art.png" /><link rel="stylesheet" href="/style.css" />
<link rel="manifest" href="/site.webmanifest" /></head><body><main id="start"><a href="/guides/g.html#open">Open</a>
<picture><source srcset="/small.webp 1x, /large.webp 2x" /><img src="/art.png" width="8" height="8" alt="A drawing" /></picture>
<script src="/download.js"></script></main></body></html>"##;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("diacraft-site-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        let site = Self(path);
        for (name, content) in [
            ("index.html", HTML),
            ("guides/g.html", &HTML.replace("invalid/\"", "invalid/guides/g\"").replace("id=\"start\"", "id=\"open\"")),
            ("style.css", "body {background-image:url('/paper.png')}"),
            ("download.js", "const api = 'https://api.github.com/repos/';"),
            ("site.webmanifest", r#"{"start_url":"/","icons":[{"src":"/icon.png"}]}"#),
            ("sitemap.xml", r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>https://example.invalid/</loc></url></urlset>"#),
            ("_headers", HEADERS),
            ("art.png", "art"),
            ("small.webp", "small"),
            ("large.webp", "large"),
            ("icon.png", "icon"),
            ("paper.png", "paper"),
        ] {
            site.write(name, content);
        }
        site
    }

    fn write(&self, name: &str, content: impl AsRef<[u8]>) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn rejects(&self, diagnostic: &str) {
        let err = check(&self.0, DOMAIN).unwrap_err();
        assert!(err.contains(diagnostic), "expected {diagnostic:?} in {err:?}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn valid_site_resolves_fragments_and_counts_shared_assets_once() {
    let site = Fixture::new();
    site.write("art.png", vec![0; 960_000]);
    site.write(
        "guides/g.html",
        HTML.replace("invalid/\"", "invalid/guides/g\"")
            .replace("id=\"start\"", "id=\"open\"")
            .replace("/guides/g.html#open", "../index.html?view=light&amp;size=s#start"),
    );
    check(&site.0, DOMAIN).unwrap();
}

#[test]
fn malformed_page_and_missing_head_metadata_fail() {
    let site = Fixture::new();
    for (from, to, diagnostic) in [
        ("</main>", "</aside>", "XML"),
        ("<title>Drawing</title>", "<title> </title>", "title"),
        ("name=\"description\"", "name=\"keywords\"", "description"),
        ("invalid/\" />", "invalid/wrong\" />", "canonical"),
        ("<html lang=\"en\">", "<html lang=\"en\" lang=\"it\">", "XML"),
    ] {
        site.write("index.html", HTML.replace(from, to));
        site.rejects(diagnostic);
    }
}

#[test]
fn markup_location_for_invalid_xml_points_to_the_page_and_input_line() {
    let site = Fixture::new();
    for bad in [
        "<1bad/>",
        "<p>bad\u{1}text</p>",
        "<p title=\"bad\u{7}attribute\"/>",
        "<p>&#1;</p>",
        "<p>&#xD800;</p>",
        "<p title=\"&#1114112;\"/>",
        "<p>&unknown;</p>",
        "<p class=\"first\" class=\"second\"/>",
        "<p></span>",
        "<p></p></p>",
        "<p title=\"ok\" bad$name=\"v\"/>",
    ] {
        site.write("index.html", HTML.replace("<picture>", &format!("{bad}<picture>")));
        site.rejects("index.html: line 5: XML");
    }
    site.write("index.html", format!("{HTML}\n<html/>"));
    site.rejects("index.html: line 7: XML");
    site.write("index.html", HTML.trim_end_matches("</html>"));
    site.rejects("index.html: line 6: XML");
    let mut bad_encoding = HTML.as_bytes().to_owned();
    let offset = HTML.find("<picture>").unwrap();
    bad_encoding[offset] = 0xff;
    site.write("index.html", bad_encoding);
    site.rejects("index.html: line 5: XML");
}

#[test]
fn reference_spellings_in_comments_and_cdata_are_literal() {
    let site = Fixture::new();
    site.write("index.html", HTML.replace("<picture>", "<!-- &#xD800; --><?note &#1114112;?><p><![CDATA[&#1114112;]]></p><picture>"));
    check(&site.0, DOMAIN).unwrap();
}

#[test]
fn markup_location_for_invalid_html_points_to_the_element() {
    let site = Fixture::new();
    for (from, to, expected) in [
        ("width=\"8\"", "width=\"-2\"", "line 5:"),
        ("<picture>", "<head/><picture>", "line 5:"),
        ("<picture>", "<main id=\"start\"/><picture>", "line 5:"),
        ("<script src=", "<base href=\"/\"/><script src=", "line 6:"),
    ] {
        site.write("index.html", HTML.replace(from, to));
        site.rejects(&format!("index.html: {expected}"));
    }
}

#[test]
fn markup_location_is_reported_by_the_site_command_before_generation() {
    // The site folder is in a repository root, which holds the workspace's version: the pages are made for that release.
    let root = Fixture::new();
    root.write("Cargo.toml", "[workspace.package]\nversion = \"0.2.0\"\n");
    let site = root.0.join("site");
    // A draft: every download has a setting, and none has a name yet.
    let assets: Vec<String> = super::settings::DOWNLOADS.iter().map(|id| format!(r#""{id}":"""#)).collect();
    root.write(
        "site/site.json",
        format!(
            r#"{{"domain":"https://example.invalid","app_url":"https://app.example.invalid","repository":"owner/repo","assets":{{{}}}}}"#,
            assets.join(",")
        ),
    );
    root.write("site/index.html", HTML.replace("<link rel=\"canonical\"", "<link 2bad=\"y\""));
    for args in [&[][..], &["--write"][..]] {
        let error = super::run(&site, args).unwrap_err();
        assert!(error.contains("index.html: line 2: XML"), "{error}");
    }
}

#[test]
fn images_need_alt_and_positive_dimensions() {
    let site = Fixture::new();
    for attr in ["alt=\"A drawing\"", "width=\"8\"", "height=\"8\""] {
        site.write("index.html", HTML.replace(attr, ""));
        site.rejects("img");
    }
    site.write("index.html", HTML.replace("width=\"8\"", "width=\"0\""));
    site.rejects("img");
}

#[test]
fn links_fragments_and_picture_candidates_must_exist() {
    let site = Fixture::new();
    for (from, to, diagnostic) in [
        ("g.html#open", "g.html#missing", "#missing"),
        ("g.html#open", "lost.html#open", "lost.html"),
        ("large.webp", "absent.webp", "absent.webp"),
        ("g.html#open", "../../outside.html", "outside.html"),
    ] {
        site.write("index.html", HTML.replace(from, to));
        site.rejects(diagnostic);
    }
}

#[test]
fn every_text_file_and_url_attribute_is_checked_for_foreign_hosts() {
    for (name, body) in [
        ("download.js", "fetch('https://api.github.com.unlisted.invalid/users')"),
        ("style.css", "body {background:url(//unlisted.invalid/p.png)}"),
        ("site.json", r#"{"domain":"https://unlisted.invalid/"}"#),
        ("robots.txt", "Sitemap: https://unlisted.invalid/site.xml"),
        ("site.webmanifest", r#"{"icons":[{"src":"https://github.com@unlisted.invalid/x"}]}"#),
        ("sitemap.xml", r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><loc>http://www.sitemaps.org/tracker</loc></urlset>"#),
    ] {
        let site = Fixture::new();
        site.write(name, body);
        site.rejects(name);
    }
    let site = Fixture::new();
    site.write("index.html", HTML.replace("/guides/g.html#open", "java&#115;cript:alert(1)"));
    site.rejects("index.html");
}

#[test]
fn vendor_and_suite_bans_apply_to_names_and_content() {
    let site = Fixture::new();
    for term in xtask::brands::DENY.iter().chain(xtask::brands::BANNED_NAMES) {
        site.write("about.txt", format!("A {} guide", term.to_uppercase()));
        site.rejects("about.txt");
    }
    site.write("about.txt", "ordinary words: revisions, supervision");
    let term = xtask::brands::DENY.first().unwrap();
    site.write(&format!("{term}-guide.txt"), "example");
    site.rejects("-guide.txt");
}

/// The owner, 2026-11-23: a page may hold the uses the brands check allows it (its format list and the trademark line), and only
/// those: the product's name in other words of the same page, in a page that is allowed none, or in a file name, is refused as before. Text
/// read through its character references holds the same uses.
#[test]
fn a_page_holds_the_format_list_and_the_trademark_line_it_is_allowed_and_no_other_name() {
    let site = Fixture::new();
    let [list, trademark] = ["site/index.html", "site/privacy.html"].map(|page| xtask::brands::allowed_in(page)[0]);
    assert_eq!(trademark, xtask::brands::TRADEMARK);
    let page =
        |body: &str| HTML.replace("<a href=\"/guides/g.html#open\">Open</a>", &format!("<a href=\"/guides/g.html#open\">Open</a><p>{body}</p>"));
    site.write("index.html", page(&format!("{list} (<code>.vsdx</code>). {trademark}")));
    check(&site.0, DOMAIN).unwrap();
    // The same words through a character reference elsewhere on the page: the text is read decoded, and still holds them.
    site.write("index.html", page(&format!("{list} &#40;<code>.vsdx</code>&#41;. {trademark}")));
    check(&site.0, DOMAIN).unwrap();
    // The name in other words of the page.
    let name = &list[..list.find(' ').unwrap()];
    for other in [format!("{list}. Made for {name}."), format!("{} editor", name.to_lowercase()), format!("{trademark} {name} files")] {
        site.write("index.html", page(&other));
        site.rejects("banned name");
    }
    // A use is allowed in its own page only: a guide that is not listed has none.
    site.write("index.html", HTML);
    let guide = HTML.replace("invalid/\"", "invalid/guides/g\"").replace("id=\"start\"", "id=\"open\"");
    site.write("guides/g.html", guide.replace("<main id=\"open\">", &format!("<main id=\"open\"><p>{list}</p>")));
    site.rejects("banned name");
    // And never in a file name.
    site.write("guides/g.html", &guide);
    site.write(&format!("{list}.txt"), "example");
    site.rejects("banned name");
}

#[test]
fn page_budget_includes_transitive_and_responsive_assets() {
    for asset in ["large.webp", "icon.png", "paper.png", "download.js"] {
        let site = Fixture::new();
        site.write(asset, vec![b' '; 999_999]);
        site.rejects("1000000");
    }
}

#[test]
fn dependencies_of_styles_and_manifest_are_resolved() {
    let site = Fixture::new();
    site.write("style.css", "@import url('/other.css');");
    site.rejects("other.css");
    site.write("style.css", "body {}");
    site.write("site.webmanifest", r#"{"icons":[{"src":"/missing-icon.png"}]}"#);
    site.rejects("missing-icon.png");
}

#[test]
fn encoded_html_text_and_attributes_obey_the_same_brand_rules() {
    let site = Fixture::new();
    let term = xtask::brands::DENY.first().unwrap();
    let encoded = term.chars().map(|c| format!("&#{};", u32::from(c))).collect::<String>();
    for phrase in ["<title>Drawing</title>", "alt=\"A drawing\""] {
        site.write("index.html", HTML.replace(phrase, &phrase.replace("Drawing", &encoded).replace("A drawing", &encoded)));
        site.rejects("banned name");
    }
}

#[test]
fn protocol_relative_script_urls_cannot_point_at_a_single_label_host() {
    let site = Fixture::new();
    site.write("download.js", "fetch('//intranet/files')");
    site.rejects("intranet");
}

#[test]
fn headers_require_global_strict_policy_and_security_headers() {
    let site = Fixture::new();
    for (from, to, diagnostic) in [
        ("/*", "/assets/*", "_headers"),
        ("'self'; script-src", "'self' *; script-src", "default-src"),
        ("script-src 'self'", "script-src 'self' 'unsafe-inline'", "script-src"),
        ("nosniff", "unused", "x-content-type-options"),
        ("Referrer-Policy", "Other-Header", "referrer-policy"),
        ("Permissions-Policy", "Other-Header", "permissions-policy"),
    ] {
        site.write("_headers", HEADERS.replace(from, to));
        site.rejects(diagnostic);
    }
}

/// A font of the site: a WOFF2 file named by its hash in `assets/hashed/`, read as bytes and never as text, counted in the weight of the pages
/// whose stylesheet names it, and allowed by the policy in so many words.
#[test]
fn a_font_is_a_hashed_woff2_file_that_the_policy_names() {
    // Bytes that are no UTF-8, as a font's are.
    let font = [&[0x77, 0x4f, 0x46, 0x32][..], &[0xff; 200_000][..]].concat();
    let name = format!("assets/hashed/text.{}.woff2", &super::parts::sha256(&font)[..12]);
    let cached = "\n/assets/hashed/*\n  Cache-Control: public, max-age=31536000, immutable\n";
    let with_font_src = HEADERS.replace("style-src 'self';", "style-src 'self'; font-src 'self';") + cached;
    let plain_style = "body {background-image:url('/paper.png')}";
    let font_style = format!("@font-face {{ font-family: Text; src: url(\"/{name}\") format(\"woff2\") }} {plain_style}");
    let site = Fixture::new();
    site.write(&name, &font);
    site.write("style.css", &font_style);

    // The policy has to say that fonts come from the site itself.
    site.write("_headers", HEADERS.to_owned() + cached);
    site.rejects("font-src 'self'");
    site.write("_headers", &with_font_src);
    check(&site.0, DOMAIN).unwrap();

    // The font counts in the weight of every page whose stylesheet names it, and of no other.
    site.write("art.png", vec![0; 850_000]);
    site.rejects("budget");
    site.write("style.css", plain_style);
    check(&site.0, DOMAIN).unwrap();
    site.write("style.css", &font_style);
    site.write("art.png", "art");

    // A font anywhere else, or of another kind, is refused: it would not be named by its hash, so it could not be kept.
    site.write("fonts/text.woff2", &font);
    site.rejects("fonts/text.woff2: a font is a .woff2 file in assets/hashed/");
    fs::remove_file(site.0.join("fonts/text.woff2")).unwrap();
    for kind in ["ttf", "otf", "woff", "eot"] {
        let other = format!("assets/hashed/text.{}.{kind}", &super::parts::sha256(&font)[..12]);
        site.write(&other, &font);
        site.rejects("a font is a .woff2 file in assets/hashed/");
        fs::remove_file(site.0.join(&other)).unwrap();
    }

    // The policy may not let fonts come from anywhere else.
    site.write("_headers", with_font_src.replace("font-src 'self'", "font-src 'self' https://fonts.example"));
    site.rejects("font-src");
}
