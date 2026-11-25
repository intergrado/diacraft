//! What the pages of the website give to search engines and to the cards of social sites, read from the files as they are in the repository: the `twitter:*` tags, the structured data
//! (JSON-LD), the dates of the sitemap, the web app's title and description. The generator and its checks are `xtask/src/site/seo.rs` (and its tests); these hold the result, so that a page
//! edited by hand and not made again, or a generator that drifts, is seen by the plain test run.

use std::collections::BTreeSet;

use serde_json::Value;

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The page files of the website, as paths from the repository's root, sorted.
fn pages() -> Vec<String> {
    let mut pages = Vec::new();
    for dir in ["site", "site/guides"] {
        for entry in std::fs::read_dir(xtask::root().join(dir)).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.ends_with(".html") {
                pages.push(format!("{dir}/{name}"));
            }
        }
    }
    pages.sort();
    assert_eq!(pages.len(), 15, "the pages, the guides, their index and 404: {pages:?}");
    pages
}

/// The `content` of the `<meta>` tag of a page with `attr="value"` (`property="og:title"`), as it is written in the page.
fn meta(html: &str, attr: &str, value: &str) -> Option<String> {
    let needle = format!("<meta {attr}=\"{value}\" content=\"");
    let rest = html.split(&needle).nth(1)?;
    Some(rest.split('"').next()?.to_owned())
}

/// The blocks of structured data of a page, parsed.
fn json_ld(html: &str) -> Vec<Value> {
    let open = "<script type=\"application/ld+json\" data-generated=\"\">";
    html.split(open).skip(1).map(|piece| serde_json::from_str(piece.split("</script>").next().unwrap()).expect("JSON-LD that parses")).collect()
}

/// The workspace's version and licence expression.
fn workspace() -> (String, String) {
    let manifest: toml::Table = read("Cargo.toml").parse().unwrap();
    let package = &manifest["workspace"]["package"];
    (package["version"].as_str().unwrap().to_owned(), package["license"].as_str().unwrap().to_owned())
}

/// The properties schema.org lists for each type the site uses, with those it inherits (from <https://schema.org/SoftwareApplication>, `/WebApplication`, `/TechArticle`, `/WebPage`, `/WebSite`
/// and `/Offer`, which were read when this was written; a `CollectionPage` is a `WebPage`, and the site gives it no property of its own): a key that is not here is a typo or a property of another type, and a search engine would ignore it.
fn properties(kind: &str) -> &'static [&'static str] {
    match kind {
        "SoftwareApplication" => &[
            "@context",
            "@type",
            "applicationCategory",
            "description",
            "downloadUrl",
            "image",
            "license",
            "name",
            "offers",
            "operatingSystem",
            "sameAs",
            "softwareVersion",
            "url",
        ],
        "WebApplication" => &[
            "@context",
            "@type",
            "applicationCategory",
            "browserRequirements",
            "description",
            "image",
            "name",
            "offers",
            "operatingSystem",
            "sameAs",
            "url",
        ],
        "WebSite" => &["@context", "@type", "name", "url"],
        "TechArticle" => &["@context", "@type", "description", "headline", "image", "inLanguage", "isPartOf", "url"],
        "WebPage" | "CollectionPage" => &["@context", "@type", "description", "inLanguage", "isPartOf", "name", "url"],
        "Offer" => &["@type", "price", "priceCurrency"],
        other => panic!("{other} is a type this test does not know: say which properties schema.org lists for it"),
    }
}

/// Every object of `value` (and each of its nested objects that has a `@type`) has only properties its type lists.
fn only_listed_properties(value: &Value, page: &str) {
    let Some(object) = value.as_object() else { return };
    if let Some(kind) = object.get("@type").and_then(Value::as_str) {
        for key in object.keys() {
            assert!(properties(kind).contains(&key.as_str()), "{page}: `{key}` is not a property that {kind} has");
        }
    }
    for nested in object.values() {
        match nested {
            Value::Object(_) => only_listed_properties(nested, page),
            Value::Array(items) => items.iter().for_each(|item| only_listed_properties(item, page)),
            _ => {}
        }
    }
}

/// Every string in `value`: the addresses are checked as a set.
fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

#[test]
fn every_page_has_twitter_tags_that_are_its_social_tags() {
    for page in pages() {
        let html = read(&page);
        assert_eq!(meta(&html, "name", "twitter:card").as_deref(), Some("summary_large_image"), "{page}");
        for (twitter, og) in [
            ("twitter:title", "og:title"),
            ("twitter:description", "og:description"),
            ("twitter:image", "og:image"),
            ("twitter:image:alt", "og:image:alt"),
        ] {
            let want = meta(&html, "property", og).unwrap_or_else(|| panic!("{page} has no {og}"));
            assert_eq!(meta(&html, "name", twitter).as_deref(), Some(want.as_str()), "{page}: {twitter} is {og}");
            assert_eq!(html.matches(&format!("name=\"{twitter}\"")).count(), 1, "{page}: one {twitter}");
        }
        // They are in the head, with the social tags: before the first element of the body.
        let head = html.split("</head>").next().unwrap();
        assert!(head.contains("name=\"twitter:card\""), "{page}");
        assert_eq!(
            meta(&html, "name", "twitter:image").as_deref(),
            Some("https://diacraft.dev/assets/social.jpg"),
            "{page}: the share picture is the site's"
        );
    }
}

#[test]
fn every_page_but_the_missing_one_has_structured_data_of_the_kind_its_place_gives_it() {
    let (version, _) = workspace();
    for page in pages() {
        let html = read(&page);
        let data = json_ld(&html);
        let kinds: Vec<&str> = data.iter().map(|d| d["@type"].as_str().unwrap()).collect();
        let expected: &[&str] = match page.as_str() {
            "site/index.html" => &["SoftwareApplication", "WebSite"],
            "site/404.html" => &[],
            "site/guides/index.html" => &["CollectionPage"],
            p if p.starts_with("site/guides/") => &["TechArticle"],
            _ => &["WebPage"],
        };
        assert_eq!(kinds, expected, "{page}");
        let canonical = meta_link(&html, "canonical");
        for document in &data {
            assert_eq!(document["@context"], "https://schema.org", "{page}");
            only_listed_properties(document, &page);
            // The text is the page's own, and an address is an HTTPS address of the site or of GitHub's repository: nothing is loaded or named from anywhere else.
            let mut found = Vec::new();
            strings(document, &mut found);
            for text in found.iter().filter(|t| t.contains("://")) {
                assert!(
                    text.starts_with("https://diacraft.dev/")
                        || text.starts_with("https://github.com/intergrado/diacraft")
                        || *text == "https://schema.org",
                    "{page}: {text}"
                );
            }
            // Nothing the site does not say: no rating, review, author or date in the data (the date of a page is in the sitemap).
            for key in
                ["aggregateRating", "review", "author", "creator", "publisher", "datePublished", "dateModified", "ratingValue", "reviewCount", "isbn"]
            {
                assert!(document.get(key).is_none(), "{page}: invented data: {key}");
            }
            if document["@type"] != "WebSite" && document["@type"] != "SoftwareApplication" {
                assert_eq!(document["url"], canonical.as_str(), "{page}: the address of the page");
            }
            if document["@type"] != "WebSite" {
                assert_eq!(
                    document["description"],
                    meta(&html, "name", "description").unwrap().replace("&amp;", "&").as_str(),
                    "{page}: the page's own description"
                );
            }
        }
        if page == "site/index.html" {
            let app = &data[0];
            assert_eq!(app["name"], "DiaCraft");
            assert_eq!(app["url"], "https://diacraft.dev/");
            assert_eq!(app["applicationCategory"], "DesignApplication");
            assert_eq!(app["operatingSystem"], "Windows, macOS, Linux, Web browser");
            assert_eq!(app["offers"]["price"], "0");
            assert_eq!(app["offers"]["priceCurrency"], "USD");
            assert_eq!(app["softwareVersion"], version.as_str(), "the version is the workspace's");
            assert_eq!(app["downloadUrl"], format!("https://github.com/intergrado/diacraft/releases/tag/v{version}").as_str());
            assert_eq!(app["image"], "https://diacraft.dev/assets/social.jpg");
            assert_eq!(app["sameAs"][0], "https://github.com/intergrado/diacraft");
            assert_eq!(data[1]["name"], "DiaCraft");
            assert_eq!(data[1]["url"], "https://diacraft.dev/");
        }
    }
}

/// The `href` of the `<link rel="...">` of a page.
fn meta_link(html: &str, rel: &str) -> String {
    let rest = html.split(&format!("<link rel=\"{rel}\" href=\"")).nth(1).expect("the link");
    rest.split('"').next().unwrap().to_owned()
}

/// The licences the home page names are the repository's own files, one for each licence in `Cargo.toml`.
#[test]
fn the_licences_the_home_page_names_are_the_files_of_the_repository_and_the_licences_of_cargo_toml() {
    let (_, expression) = workspace();
    let app = &json_ld(&read("site/index.html"))[0];
    let licences: Vec<&str> = app["license"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
    let ids: Vec<&str> = expression.split(" OR ").collect();
    assert_eq!(licences.len(), ids.len(), "one licence file for each of {expression}");
    for (id, address) in ids.iter().zip(&licences) {
        let file = match *id {
            "MIT" => "LICENSE-MIT",
            "Apache-2.0" => "LICENSE-APACHE",
            other => panic!("the licence {other} has no file in this test: add it with the file that holds its text"),
        };
        assert_eq!(*address, format!("https://github.com/intergrado/diacraft/blob/HEAD/{file}"));
        assert!(xtask::root().join(file).is_file(), "{file} is a file of the repository");
        assert!(read(file).len() > 500, "{file} holds the licence text");
    }
}

#[test]
fn the_sitemaps_dates_are_dates_that_exist_not_in_the_future_or_the_pages_are_listed_without_them() {
    let sitemap = read("site/sitemap.xml");
    let mut urls = 0;
    let today = jiff::tz::Offset::UTC.to_datetime(jiff::Timestamp::now()).date();
    for entry in sitemap.split("<url>").skip(1) {
        urls += 1;
        let loc = entry.split("<loc>").nth(1).and_then(|s| s.split("</loc>").next()).expect("a loc");
        assert!(loc.starts_with("https://diacraft.dev/"), "{loc}");
        assert!(entry.matches("<lastmod>").count() <= 1, "{loc}: one lastmod at most");
        if let Some(date) = entry.split("<lastmod>").nth(1).and_then(|s| s.split("</lastmod>").next()) {
            // `YYYY-MM-DD` and no more (no time, no zone), and a day that exists.
            assert!(date.len() == 10 && date.parse::<jiff::civil::Date>().is_ok_and(|d| d.to_string() == date), "{loc}: `{date}` is not a date");
            let day: jiff::civil::Date = date.parse().unwrap();
            assert!(day >= jiff::civil::date(2026, 1, 1), "{loc}: {date} is before the site existed");
            assert!(day <= today.tomorrow().unwrap(), "{loc}: {date} is in the future");
        }
    }
    assert_eq!(urls, 14, "every page but 404");
    // The robots file lets every crawler in and names the sitemap.
    assert_eq!(read("site/robots.txt"), "User-agent: *\nAllow: /\n\nSitemap: https://diacraft.dev/sitemap.xml\n");
}

/// The web app's page (the source of what `cargo xtask site --web` makes the app's start page from) has its own title and a description a search engine shows whole.
#[test]
fn the_web_apps_page_has_its_own_title_and_a_description_of_a_length_a_result_shows() {
    let page = read("apps/diacraft-web/index.html");
    let title = page.split("<title>").nth(1).and_then(|s| s.split("</title>").next()).unwrap();
    assert_eq!(title, "DiaCraft: free online diagram editor");
    assert!(title.len() <= 60, "a title over 60 characters is cut in a result: {title}");
    let description = meta(&page, "name", "description").expect("the app's page has a description");
    assert!((50..=160).contains(&description.chars().count()), "{} characters: {description}", description.chars().count());
    // It is the site's own words, and says what the page says of the app (`site/browser.html`).
    for word in ["browser", "nothing to install", "VSDX", "OpenDocument", "your computer"] {
        assert!(description.contains(word), "the description does not say `{word}`: {description}");
    }
    // The app's page has none of the uses of a vendor's name that the owner allows in other places (the file lists of the home page): the names the repository denies are not in it.
    let lower = description.to_lowercase();
    assert!(!xtask::brands::BANNED_NAMES.iter().any(|term| lower.contains(*term)), "{description}");
    assert!(!lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|word| xtask::brands::DENY.contains(&word)), "{description}");
    // The tags the build adds are made from these two and from `site/site.json` (`xtask/src/site/seo.rs`): none is written by hand in the source.
    for made in ["canonical", "og:", "twitter:", "ld+json"] {
        assert!(!page.contains(&format!("\"{made}")) && !page.contains(&format!("=\"{made}")), "the source page holds {made}");
    }
}

/// The app's page has the one landmark a screen reader jumps to (Lighthouse's `landmark-one-main`), and it is the size of the page, so that the canvas in it still fills the window: the canvas
/// is looked up by its id and sized by the page's CSS, and neither moves.
#[test]
fn the_web_apps_page_has_one_main_landmark_that_holds_the_canvas_and_is_the_size_of_the_page() {
    let page = read("apps/diacraft-web/index.html");
    assert_eq!(page.matches("<main>").count(), 1);
    assert_eq!(page.matches("</main>").count(), 1);
    let main = page.split("<main>").nth(1).and_then(|s| s.split("</main>").next()).unwrap();
    assert!(
        main.contains("<canvas id=\"diacraft_canvas\"></canvas>") && main.contains("id=\"diacraft_loading\""),
        "the canvas and the loading text are in it"
    );
    let css = read("apps/diacraft-web/page.css");
    let rule = css.lines().find(|l| l.starts_with("main {")).expect("a rule for main");
    assert!(rule.contains("width: 100%") && rule.contains("height: 100%") && rule.contains("display: block"), "{rule}");
}

/// Each page has a title of at most 60 characters that leads with what the page is about, and a description of 140 to 160 characters, the lengths a result shows whole (the owner,
/// 2026-11-25), and no two pages share either. The page for a missing address is not listed by a search engine and is left out.
#[test]
fn every_page_has_its_own_title_and_a_description_of_a_length_a_result_shows_whole() {
    let (mut titles, mut descriptions) = (BTreeSet::new(), BTreeSet::new());
    for page in pages().into_iter().filter(|page| page != "site/404.html") {
        let html = read(&page);
        let title = html.split("<title>").nth(1).and_then(|s| s.split("</title>").next()).unwrap().replace("&amp;", "&");
        assert!(title.chars().count() <= 60, "{page}: a title of {} characters: {title}", title.chars().count());
        assert_eq!(
            meta(&html, "property", "og:title").map(|t| t.replace("&amp;", "&")).as_deref(),
            Some(title.as_str()),
            "{page}: og:title is the title"
        );
        let description = meta(&html, "name", "description").unwrap().replace("&amp;", "&");
        let count = description.chars().count();
        assert!((140..=160).contains(&count), "{page}: a description of {count} characters: {description}");
        assert!(titles.insert(title.clone()), "{page}: the title {title} is another page's");
        assert!(descriptions.insert(description.clone()), "{page}: the description is another page's");
    }
}
