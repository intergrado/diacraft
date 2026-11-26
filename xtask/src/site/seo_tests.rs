//! Tests of what the pages give to search engines and social cards: the `twitter:*` tags, the structured data, the dates of the sitemap, and the checks that read them.

use super::app_tests::Dir;
use super::html::{self, Head};
use super::seo::{self, Dates, Kind};
use super::settings::{self, DOWNLOADS, DateSource, Settings};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;

fn settings() -> Settings {
    Settings {
        domain: "https://drawing.example".into(),
        app_url: "https://app.example".into(),
        repository: "owner/drawing".into(),
        assets: DOWNLOADS.iter().map(|id| ((*id).to_owned(), format!("{id}-{{tag}}.bin"))).collect(),
        version: "0.2.0".into(),
        licences: vec!["LICENSE-MIT".into(), "LICENSE-APACHE".into()],
        comparison: None,
    }
}

/// A page as the pages of the site are written: one tag to a line, the canonical address and the `og:*` tags in place, and `</head>` on a line of its own.
fn page(title: &str, description: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>{title}</title>
  <meta name="description" content="{description}" />
  <link rel="canonical" href="https://drawing.example/x" />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content="DiaCraft" />
  <meta property="og:title" content="{title}" />
  <meta property="og:description" content="{description}" />
  <meta property="og:url" content="https://drawing.example/x" />
  <meta property="og:image" content="https://drawing.example/assets/social.jpg" />
  <meta property="og:image:alt" content="A drawing of an octopus." />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <meta name="theme-color" content="#efe9dc" />
  <link rel="stylesheet" href="/styles.css" />
</head>
<body><main id="main"><p>Hello</p></main></body>
</html>
"##
    )
}

const GUIDE: &str = "Draw your first diagram · DiaCraft";

fn rendered(name: &str) -> String {
    let source = page(GUIDE, "Build and label a two-step flowchart.");
    seo::render(&source, &settings(), name, "https://drawing.example/guides/one").unwrap()
}

/// The text of the blocks of structured data of a page, parsed.
fn blocks(page: &str) -> Vec<Value> {
    let doc = roxmltree::Document::parse_with_options(page, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).unwrap();
    doc.descendants()
        .filter(|n| n.has_tag_name("script") && n.attribute("type") == Some("application/ld+json"))
        .map(|n| serde_json::from_str(n.text().unwrap()).unwrap())
        .collect()
}

#[test]
fn the_twitter_tags_are_the_og_tags_and_follow_the_last_og_tag() {
    let out = rendered("guides/one.html");
    let lines: Vec<&str> = out.lines().collect();
    let at = lines.iter().position(|l| l.contains("twitter:card")).expect("a twitter:card tag");
    assert_eq!(
        &lines[at..at + 5],
        [
            r#"  <meta name="twitter:card" content="summary_large_image" />"#,
            r#"  <meta name="twitter:title" content="Draw your first diagram · DiaCraft" />"#,
            r#"  <meta name="twitter:description" content="Build and label a two-step flowchart." />"#,
            r#"  <meta name="twitter:image" content="https://drawing.example/assets/social.jpg" />"#,
            r#"  <meta name="twitter:image:alt" content="A drawing of an octopus." />"#,
        ]
    );
    assert!(lines[at - 1].contains("og:image:height"), "after the last og: tag: {}", lines[at - 1]);
    assert!(lines[at + 5].contains("theme-color"), "and before what followed it: {}", lines[at + 5]);
}

#[test]
fn a_value_with_markup_characters_is_escaped_in_the_tag_and_not_changed() {
    let source = page("Tom &amp; Jerry &lt;3 &quot;q&quot;", "Tom &amp; Jerry &lt;3 &quot;q&quot;");
    let out = seo::render(&source, &settings(), "guides/one.html", "https://drawing.example/guides/one").unwrap();
    assert!(out.contains(r#"<meta name="twitter:title" content="Tom &amp; Jerry &lt;3 &quot;q&quot;" />"#), "{out}");
    // The page is still well-formed, and the data holds the text as it reads, with no markup character in the script.
    let head: Head = html::head(&out).unwrap();
    assert_eq!(head.title, "Tom & Jerry <3 \"q\"");
    let data = blocks(&out);
    assert_eq!(data[0]["headline"], "Tom & Jerry <3 \"q\"");
    assert_eq!(data[0]["description"], "Tom & Jerry <3 \"q\"");
    let script = out.split(r#"<script type="application/ld+json" data-generated="">"#).nth(1).and_then(|s| s.split("</script>").next()).unwrap();
    assert!(!script.contains(['<', '>', '&']), "{script}");
    assert!(script.contains("\\u0026") && script.contains("\\u003c"), "{script}");
}

/// A line break, a carriage return or a tab in a value (`&#10;`, `&#13;`, `&#9;` in the `og:` tag, which a reader of the page turns into the character) is written back as the reference: raw, the
/// tag would run over two lines, and a page made again would take away the first line of it only.
#[test]
fn a_line_break_or_a_tab_in_a_value_is_written_as_a_reference_and_the_page_is_made_the_same_every_time() {
    let source = page("A&#10;B&#9;C", "D&#13;E&#10;F");
    let url = "https://drawing.example/guides/one";
    let once = seo::render(&source, &settings(), "guides/one.html", url).unwrap();
    for tag in [r#"<meta name="twitter:title" content="A&#10;B&#9;C" />"#, r#"<meta name="twitter:description" content="D&#13;E&#10;F" />"#] {
        assert!(once.lines().any(|line| line.trim() == tag), "{tag} is not a line of {once}");
    }
    // Each tag is one line: five `twitter:` tags, and nothing of a value on a line of its own.
    assert_eq!(once.lines().filter(|line| line.contains("twitter:")).count(), 5, "{once}");
    assert!(once.lines().all(|line| !line.contains('\r')) && once.lines().all(|line| line.trim() != "B\" />"), "{once}");
    // The page is well-formed and the data holds the characters.
    let head: Head = html::head(&once).unwrap();
    assert_eq!((head.og_title.as_str(), head.og_description.as_str()), ("A\nB\tC", "D\rE\nF"));
    assert_eq!(blocks(&once)[0]["headline"], "A\nB\tC");
    // Made again, it is the same page: nothing of the first making is left over.
    assert_eq!(seo::render(&once, &settings(), "guides/one.html", url).unwrap(), once);
}

/// The same, as the command's two modes: `--write` makes the page, and the check that follows finds nothing stale.
#[test]
fn a_page_written_with_a_line_break_in_a_value_passes_the_check_that_follows() {
    let dir = Dir::new("breaks");
    dir.write("site/index.html", page("A&#10;B&#9;C", "D&#13;E&#10;F"));
    let site = dir.path().join("site");
    settings::apply(&site, &settings(), true).unwrap();
    settings::apply(&site, &settings(), false).unwrap();
    let written = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(written.contains(r#"<meta name="twitter:title" content="A&#10;B&#9;C" />"#), "{written}");
}

#[test]
fn making_a_page_again_changes_nothing_and_replaces_what_was_there() {
    let once = rendered("guides/one.html");
    let again = seo::render(&once, &settings(), "guides/one.html", "https://drawing.example/guides/one").unwrap();
    assert_eq!(once, again);
    // A block or a tag written by hand in the same form is replaced, not repeated.
    let changed = once.replace("two-step flowchart", "three-step flowchart");
    let fresh = seo::render(&changed, &settings(), "guides/one.html", "https://drawing.example/guides/one").unwrap();
    assert!(!fresh.contains("two-step") && fresh.contains("three-step"));
    assert_eq!(fresh.matches("twitter:card").count(), 1);
    assert_eq!(fresh.matches("application/ld+json").count(), 1);
}

#[test]
fn a_block_of_data_without_the_mark_is_the_pages_own_and_stays() {
    let own = "  <script type=\"application/ld+json\">\n    {\"@context\": \"https://schema.org\", \"@type\": \"FAQPage\", \"mainEntity\": []}\n  </script>\n";
    let source = page(GUIDE, "d").replace("</head>", &format!("{own}</head>"));
    let out = seo::render(&source, &settings(), "guides/one.html", "u").unwrap();
    assert!(out.contains(own), "{out}");
    assert_eq!(blocks(&out).len(), 2, "the page's own block and the generated one");
    assert_eq!(seo::render(&out, &settings(), "guides/one.html", "u").unwrap(), out);
}

#[test]
fn a_twitter_tag_of_another_name_is_the_pages_own_and_stays() {
    let source = page(GUIDE, "d")
        .replace("  <meta name=\"theme-color\"", "  <meta name=\"twitter:site\" content=\"@example\" />\n  <meta name=\"theme-color\"");
    let out = seo::render(&source, &settings(), "guides/one.html", "u").unwrap();
    assert_eq!(out.matches("twitter:site").count(), 1);
    assert_eq!(seo::render(&out, &settings(), "guides/one.html", "u").unwrap(), out);
}

#[test]
fn a_page_without_a_place_for_the_tags_or_the_data_is_refused() {
    let source = page(GUIDE, "d");
    let no_end = source.replace("</head>", "<meta name=\"x\" content=\"y\" /></head>");
    let error = seo::render(&no_end, &settings(), "guides/one.html", "u").unwrap_err();
    assert!(error.contains("</head> at the start of a line"), "{error}");
    let no_og = source.lines().filter(|l| !l.contains("og:")).collect::<Vec<_>>().join("\n");
    assert!(seo::render(&no_og, &settings(), "guides/one.html", "u").is_err());
    let open = source.replace("</head>", "  <script type=\"application/ld+json\">\n</head>");
    // A block that is not closed is not a page either.
    assert!(seo::render(&open, &settings(), "guides/one.html", "u").is_err());
}

#[test]
fn the_kind_of_a_page_is_its_place_in_the_site() {
    assert_eq!(seo::kind("index.html"), Kind::Home);
    assert_eq!(seo::kind("guides/agents.html"), Kind::Guide);
    // The guides' own index is a page that lists them, not a guide.
    assert_eq!(seo::kind("guides/index.html"), Kind::Index);
    assert_eq!(seo::kind("flowcharts.html"), Kind::Page);
    assert_eq!(seo::kind("changelog.html"), Kind::Page);
    assert_eq!(seo::kind("privacy.html"), Kind::Page);
    assert_eq!(seo::kind("browser.html"), Kind::Page);
    assert_eq!(seo::kind("404.html"), Kind::NotFound);
}

#[test]
fn the_home_page_is_a_software_application_and_a_website_with_nothing_invented() {
    let source = page("DiaCraft — Open, edit and convert drawings", "A free, open-source diagram editor.");
    let out = seo::render(&source, &settings(), "index.html", "https://drawing.example/").unwrap();
    let data = blocks(&out);
    assert_eq!(data.len(), 2);
    assert_eq!(
        data[0],
        json!({
            "@context": "https://schema.org",
            "@type": "SoftwareApplication",
            "name": "DiaCraft",
            "url": "https://drawing.example/",
            "description": "A free, open-source diagram editor.",
            "applicationCategory": "DesignApplication",
            "operatingSystem": "Windows, macOS, Linux, Web browser",
            "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
            "softwareVersion": "0.2.0",
            "downloadUrl": "https://github.com/owner/drawing/releases/tag/v0.2.0",
            "license": ["https://github.com/owner/drawing/blob/HEAD/LICENSE-MIT", "https://github.com/owner/drawing/blob/HEAD/LICENSE-APACHE"],
            "image": "https://drawing.example/assets/social.jpg",
            "sameAs": ["https://github.com/owner/drawing"],
        })
    );
    assert_eq!(data[1], json!({"@context": "https://schema.org", "@type": "WebSite", "name": "DiaCraft", "url": "https://drawing.example/"}));
    // No rating, review, author or date is in anything the generator makes.
    for key in ["aggregateRating", "review", "author", "datePublished", "dateModified", "ratingValue", "reviewCount"] {
        assert!(!out.contains(key), "{key} is invented data");
    }
}

#[test]
fn the_version_and_the_release_page_follow_the_settings_and_a_missing_licence_is_left_out() {
    let source = page("DiaCraft", "d");
    let mut moved = settings();
    moved.version = "1.4.0-rc.1".into();
    moved.licences = Vec::new();
    let out = seo::render(&source, &moved, "index.html", "https://drawing.example/").unwrap();
    let data = blocks(&out);
    assert_eq!(data[0]["softwareVersion"], "1.4.0-rc.1");
    assert_eq!(data[0]["downloadUrl"], "https://github.com/owner/drawing/releases/tag/v1.4.0-rc.1");
    assert!(data[0].get("license").is_none());
}

#[test]
fn a_guide_is_a_tech_article_and_another_page_a_web_page_and_the_missing_page_has_no_data() {
    let guide = blocks(&rendered("guides/one.html"));
    assert_eq!(
        guide,
        [json!({
            "@context": "https://schema.org",
            "@type": "TechArticle",
            "headline": "Draw your first diagram",
            "description": "Build and label a two-step flowchart.",
            "url": "https://drawing.example/guides/one",
            "image": "https://drawing.example/assets/social.jpg",
            "inLanguage": "en",
            "isPartOf": {"@type": "WebSite", "name": "DiaCraft", "url": "https://drawing.example/"},
        })]
    );
    let plain = blocks(&rendered("changelog.html"));
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0]["@type"], "WebPage");
    assert_eq!(plain[0]["name"], "Draw your first diagram");
    // The index of the guides is a collection of them, with the properties a page has.
    let index = blocks(&rendered("guides/index.html"));
    assert_eq!(
        index,
        [json!({
            "@context": "https://schema.org",
            "@type": "CollectionPage",
            "name": "Draw your first diagram",
            "description": "Build and label a two-step flowchart.",
            "url": "https://drawing.example/guides/one",
            "inLanguage": "en",
            "isPartOf": {"@type": "WebSite", "name": "DiaCraft", "url": "https://drawing.example/"},
        })]
    );
    let missing = rendered("404.html");
    assert!(blocks(&missing).is_empty() && !missing.contains("ld+json"));
    assert!(missing.contains("twitter:card"), "the missing page has the social tags");
}

#[test]
fn the_keys_of_the_data_are_in_alphabetical_order_so_that_the_text_is_the_same_on_every_build() {
    let out = rendered("guides/one.html");
    let script = out.split(r#"<script type="application/ld+json" data-generated="">"#).nth(1).unwrap();
    let keys: Vec<&str> = script.lines().filter(|l| l.starts_with("      \"")).map(|l| l.trim().split('"').nth(1).unwrap()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "a feature of serde_json that keeps the order of insertion would change every page");
    assert!(script.starts_with("\n    {\n      \"@context\": \"https://schema.org\",\n      \"@type\": \"TechArticle\","), "{script}");
}

#[test]
fn a_title_is_the_name_without_the_name_of_the_site() {
    let out = seo::render(&page("DiaCraft · DiaCraft", "d"), &settings(), "changelog.html", "u").unwrap();
    assert_eq!(blocks(&out)[0]["name"], "DiaCraft");
    let out = seo::render(&page("Just a title", "d"), &settings(), "changelog.html", "u").unwrap();
    assert_eq!(blocks(&out)[0]["name"], "Just a title");
}

#[test]
fn the_licence_files_follow_cargo_tomls_expression_and_an_unknown_licence_is_an_error() {
    assert_eq!(seo::licence_files("MIT OR Apache-2.0").unwrap(), ["LICENSE-MIT", "LICENSE-APACHE"]);
    assert_eq!(seo::licence_files("MIT").unwrap(), ["LICENSE-MIT"]);
    for bad in ["GPL-3.0", "MIT AND Apache-2.0", "", "MIT OR "] {
        assert!(seo::licence_files(bad).is_err(), "{bad:?}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the checks

#[test]
fn the_check_reads_the_structured_data_and_refuses_what_is_not_data() {
    let site = Dir::new("jsonld");
    site.write("_headers", super::app_tests::PAGES_HEADERS);
    site.write("styles.css", "body { margin: 0 }");
    site.write("assets/social.jpg", "jpg");
    let good = rendered("guides/one.html").replace("https://drawing.example/x", "https://drawing.example/");
    site.write("index.html", &good);
    let check = || super::check(site.path(), "https://drawing.example");
    check().unwrap();
    for (from, to, diagnostic) in [
        ("\"@context\": \"https://schema.org\"", "\"@context\": \"https://schema.example\"", "must have the context"),
        ("\"@type\": \"TechArticle\"", "\"@type\": \"\"", "must have a @type"),
        ("\"@type\": \"TechArticle\"", "\"@type\": \"TechArticle\"\n    ,", "not JSON"),
        ("    {\n", "    [\n", "not JSON"),
        (r#"<script type="application/ld+json" data-generated="">"#, r#"<script type="application/ld+json" src="/x.json">"#, "no src"),
    ] {
        site.write("index.html", good.replacen(from, to, 1));
        let error = check().unwrap_err();
        assert!(error.contains(diagnostic), "{from:?}: {error}");
    }
    // An array of objects is not one object: each block is one.
    site.write("index.html", good.replacen("    {\n", "    [{\n", 1).replacen("    }\n", "    }]\n", 1));
    assert!(check().unwrap_err().contains("must be one object"));
}

#[test]
fn the_schema_context_is_allowed_where_the_generator_writes_it_and_nowhere_else() {
    let origins = super::links::Origins::new("https://drawing.example", "https://app.example");
    assert!(super::links::hosts("  \"@context\": \"https://schema.org\",", &origins, "index.html").is_ok());
    for bad in ["<a href=\"https://schema.org/Thing\">x</a>", "\"@id\": \"https://schema.org\"", "\"@context\": \"https://schema.org.example\""] {
        assert!(super::links::hosts(bad, &origins, "index.html").is_err(), "{bad}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the dates

#[test]
fn a_date_is_four_digits_a_month_and_a_day_that_exist() {
    for good in ["2026-11-25", "2024-02-29", "2000-01-01", "9999-12-31"] {
        assert!(seo::valid_date(good), "{good}");
    }
    for bad in [
        "",
        "2026-11-25T00:00:00Z",
        "2026-13-01",
        "2026-02-29",
        "2026-00-10",
        "2026-11-31",
        "26-11-25",
        "2026/11/25",
        "2026-1-25",
        " 2026-11-25",
        "2026-11-25 ",
        "２０２６-11-25",
        "+026-11-25",
        "2026-11-2５",
    ] {
        assert!(!seo::valid_date(bad), "{bad:?}");
    }
}

/// Runs `git` in `dir` with the author and the committer on `date`, and fails the test when it fails.
fn git(dir: &Path, date: &str, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit(repo: &Dir, date: &str, files: &[(&str, &str)]) {
    for (name, content) in files {
        repo.write(name, content);
    }
    git(repo.path(), date, &["add", "--", "."]);
    git(repo.path(), date, &["commit", "-q", "-m", "x"]);
}

/// A repository with `site/a.html` last changed on 2026-05-04 (UTC), `site/b.html` on 2026-04-02, and `site/c.html` not committed.
fn history() -> Dir {
    let repo = Dir::new("history");
    git(repo.path(), "2026-03-01T12:00:00Z", &["init", "-q"]);
    commit(&repo, "2026-03-01T12:00:00Z", &[("site/a.html", "one"), ("site/b.html", "one")]);
    commit(&repo, "2026-04-02T12:00:00Z", &[("site/b.html", "two")]);
    // 23:30 at is 04:30 the next day in UTC: the date is the UTC one, whatever the time zone of the commit or of this machine.
    commit(&repo, "2026-05-03T23:30:00-0500", &[("site/a.html", "two")]);
    repo.write("site/c.html", "new");
    repo
}

#[test]
fn the_date_of_a_page_is_that_of_the_last_commit_that_touched_it_in_utc() {
    let repo = history();
    let files: Vec<String> = ["site/a.html", "site/b.html", "site/c.html"].map(str::to_owned).to_vec();
    let Dates::Found(found) = seo::git_dates(repo.path(), &files) else { panic!("a repository with its whole history gives dates") };
    assert_eq!(found.get("site/a.html").map(String::as_str), Some("2026-05-04"));
    assert_eq!(found.get("site/b.html").map(String::as_str), Some("2026-04-02"));
    assert_eq!(found.get("site/c.html"), None, "a file no commit has touched has no date");
}

#[test]
fn a_shallow_checkout_and_a_folder_with_no_history_give_no_date_and_say_why() {
    let repo = history();
    let clone = Dir::new("shallow");
    let url = format!("file://{}", repo.path().display());
    git(clone.path(), "2026-06-01T00:00:00Z", &["clone", "-q", "--depth", "1", &url, "checkout"]);
    let checkout = clone.path().join("checkout");
    assert!(checkout.join("site/a.html").is_file());
    match seo::git_dates(&checkout, &["site/a.html".to_owned()]) {
        Dates::Unavailable(why) => assert!(why.contains("shallow"), "{why}"),
        Dates::Found(found) => panic!("a shallow checkout gave {found:?}"),
    }
    let bare = Dir::new("nohistory");
    bare.write("site/a.html", "x");
    assert!(matches!(seo::git_dates(bare.path(), &["site/a.html".to_owned()]), Dates::Unavailable(_)));
}

/// `site/` of the history repository with two pages the generator can make (`index.html` is committed on 2026-05-04 and `guides/one.html` on 2026-04-02).
fn site_repo() -> Dir {
    let repo = Dir::new("siterepo");
    git(repo.path(), "2026-03-01T12:00:00Z", &["init", "-q"]);
    let guide = page(GUIDE, "A guide.");
    commit(&repo, "2026-04-02T12:00:00Z", &[("site/guides/one.html", &guide)]);
    commit(&repo, "2026-05-04T08:00:00Z", &[("site/index.html", &page("DiaCraft", "The home."))]);
    repo
}

#[test]
fn write_gives_each_url_the_date_of_its_page_and_a_check_keeps_them() {
    let repo = site_repo();
    let site = repo.path().join("site");
    settings::apply_with(&site, &settings(), true, &DateSource::Git(repo.path())).unwrap();
    let sitemap = fs::read_to_string(site.join("sitemap.xml")).unwrap();
    assert_eq!(
        sitemap,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n  \
         <url><loc>https://drawing.example/</loc><lastmod>2026-05-04</lastmod></url>\n  \
         <url><loc>https://drawing.example/guides/one</loc><lastmod>2026-04-02</lastmod></url>\n</urlset>\n"
    );
    // The check, which has no history to compare with in a shallow checkout, takes the dates as they are; so does the copy of the pages that a build makes.
    settings::apply(&site, &settings(), false).unwrap();
    settings::apply(&site, &settings(), true).unwrap();
    assert_eq!(fs::read_to_string(site.join("sitemap.xml")).unwrap(), sitemap);
}

#[test]
fn a_date_that_is_not_a_date_fails_the_check_and_an_absent_one_does_not() {
    let repo = site_repo();
    let site = repo.path().join("site");
    settings::apply_with(&site, &settings(), true, &DateSource::Git(repo.path())).unwrap();
    let good = fs::read_to_string(site.join("sitemap.xml")).unwrap();
    for bad in ["2026-02-30", "yesterday", "2026-05-04T08:00:00Z", ""] {
        fs::write(site.join("sitemap.xml"), good.replace("2026-05-04", bad)).unwrap();
        let error = settings::apply(&site, &settings(), false).unwrap_err();
        assert!(error.contains("is not a date"), "{bad:?}: {error}");
    }
    // A sitemap with no date at all is as good as one with them: dates are left out where history cannot give them.
    fs::write(site.join("sitemap.xml"), good.replace("<lastmod>2026-05-04</lastmod>", "").replace("<lastmod>2026-04-02</lastmod>", "")).unwrap();
    settings::apply(&site, &settings(), false).unwrap();
    // A page that is not in the sitemap, or one that is, in a different order, is a stale file, as it was.
    fs::write(
        site.join("sitemap.xml"),
        good.replace("<loc>https://drawing.example/guides/one</loc>", "<loc>https://drawing.example/guides/two</loc>"),
    )
    .unwrap();
    assert!(settings::apply(&site, &settings(), false).unwrap_err().contains("stale"));
}

#[test]
fn a_shallow_checkout_writes_the_sitemap_with_no_dates() {
    let repo = site_repo();
    let clone = Dir::new("shallowsite");
    let url = format!("file://{}", repo.path().display());
    git(clone.path(), "2026-06-01T00:00:00Z", &["clone", "-q", "--depth", "1", &url, "checkout"]);
    let checkout = clone.path().join("checkout");
    let site = checkout.join("site");
    // The sitemap has dates already: writing it in a place that cannot know them leaves them out, and does not keep or invent any.
    settings::apply_with(&site, &settings(), true, &DateSource::Keep).unwrap();
    fs::write(
        site.join("sitemap.xml"),
        fs::read_to_string(site.join("sitemap.xml")).unwrap().replace("</loc>", "</loc><lastmod>2026-01-01</lastmod>"),
    )
    .unwrap();
    settings::apply_with(&site, &settings(), true, &DateSource::Git(&checkout)).unwrap();
    let sitemap = fs::read_to_string(site.join("sitemap.xml")).unwrap();
    assert!(!sitemap.contains("lastmod"), "{sitemap}");
    assert_eq!(sitemap.matches("<url>").count(), 2);
}

#[test]
fn a_shallow_checkout_as_the_workflows_have_keeps_the_committed_dates_and_holds_them_to_their_form() {
    // What `release.yml` and `deploy-site.yml` do: check out one commit, and run `cargo xtask site` and `site --web`, which are a check and a copy that keeps the sitemap. Neither may
    // need the history, and the dates the sitemap was committed with are what is deployed.
    let repo = site_repo();
    let site = repo.path().join("site");
    settings::apply_with(&site, &settings(), true, &DateSource::Git(repo.path())).unwrap();
    let committed = fs::read_to_string(site.join("sitemap.xml")).unwrap();
    assert!(committed.contains("<lastmod>2026-05-04</lastmod>") && committed.contains("<lastmod>2026-04-02</lastmod>"));
    // The sitemap and the robots file are committed with the pages (a commit of the sitemap does not touch a page, so the pages' dates stay).
    commit(&repo, "2026-06-01T10:00:00Z", &[]);
    let clone = Dir::new("workflow");
    let url = format!("file://{}", repo.path().display());
    git(clone.path(), "2026-06-02T00:00:00Z", &["clone", "-q", "--depth", "1", &url, "checkout"]);
    let site = clone.path().join("checkout").join("site");
    // The check and the copy of the pages that a build makes find nothing stale, and change nothing.
    settings::apply(&site, &settings(), false).unwrap();
    settings::apply(&site, &settings(), true).unwrap();
    assert_eq!(fs::read_to_string(site.join("sitemap.xml")).unwrap(), committed);
}

// ------------------------------------------------------------------------------------------------------------------------ the web app

use super::app::{self};
use super::app_tests::{EMPTY_WASM, TRUNK_PAGE, out_dir_with, repo, settings as app_settings};

/// The folder the app's own site is made as, from the fixture repository's Trunk build.
fn app_folder() -> Dir {
    out_dir_with(EMPTY_WASM)
}

fn app_page(out: &Dir) -> String {
    fs::read_to_string(out.path().join("index.html")).unwrap()
}

#[test]
fn the_apps_page_has_a_title_a_description_a_canonical_address_and_the_social_tags() {
    let out = app_folder();
    let page = app_page(&out);
    for tag in [
        "<title>DiaCraft: free online diagram editor</title>",
        "<meta name=\"description\" content=\"Draw diagrams in your browser, free &amp; with nothing to install.\">",
        "<link rel=\"canonical\" href=\"https://app.example.invalid/\">",
        "<meta property=\"og:type\" content=\"website\">",
        "<meta property=\"og:site_name\" content=\"DiaCraft\">",
        "<meta property=\"og:title\" content=\"DiaCraft: free online diagram editor\">",
        "<meta property=\"og:description\" content=\"Draw diagrams in your browser, free &amp; with nothing to install.\">",
        "<meta property=\"og:url\" content=\"https://app.example.invalid/\">",
        "<meta property=\"og:image\" content=\"https://app.example.invalid/social.jpg\">",
        "<meta property=\"og:image:alt\" content=\"DiaCraft diagramming software, illustrated with an octopus connecting shapes.\">",
        "<meta property=\"og:image:width\" content=\"1200\">",
        "<meta property=\"og:image:height\" content=\"630\">",
        "<meta name=\"twitter:card\" content=\"summary_large_image\">",
        "<meta name=\"twitter:title\" content=\"DiaCraft: free online diagram editor\">",
        "<meta name=\"twitter:description\" content=\"Draw diagrams in your browser, free &amp; with nothing to install.\">",
        "<meta name=\"twitter:image\" content=\"https://app.example.invalid/social.jpg\">",
    ] {
        assert!(page.contains(tag), "{tag} is not in {page}");
    }
    // The tags are in the head, and the page still starts as it did: the loader by `src`, the preloads, the canvas.
    let head = page.split("</head>").next().unwrap();
    assert!(
        head.contains("rel=\"canonical\"")
            && head.contains("application/ld+json")
            && head.contains("<script type=\"module\" src=\"./start.js\"></script>")
    );
    assert!(page.contains("<canvas id=\"diacraft_canvas\"></canvas>") && page.contains("rel=\"preload\" href=\"./manifest.json\""), "{page}");
    // The data is a free web application with what it needs of a browser, and no ratings.
    let open = "<script type=\"application/ld+json\" data-generated=\"\">";
    let data: Value = serde_json::from_str(page.split(open).nth(1).unwrap().split("</script>").next().unwrap()).unwrap();
    assert_eq!(
        data,
        json!({
            "@context": "https://schema.org",
            "@type": "WebApplication",
            "name": "DiaCraft",
            "url": "https://app.example.invalid/",
            "description": "Draw diagrams in your browser, free & with nothing to install.",
            "applicationCategory": "DesignApplication",
            "operatingSystem": "Any",
            "browserRequirements": "Requires a recent desktop browser with WebAssembly and WebGPU or WebGL 2.",
            "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
            "image": "https://app.example.invalid/social.jpg",
            "sameAs": ["https://github.com/owner/drawing"],
        })
    );
    assert_eq!(page.matches(open).count(), 1);
    assert!(!page.contains("aggregateRating") && !page.contains("review"));
    app::check(out.path()).unwrap();
}

#[test]
fn the_apps_folder_has_a_robots_file_that_lets_everyone_in_a_one_address_sitemap_and_its_own_copy_of_the_picture() {
    let out = app_folder();
    assert_eq!(
        fs::read_to_string(out.path().join("robots.txt")).unwrap(),
        "User-agent: *\nAllow: /\n\nSitemap: https://app.example.invalid/sitemap.xml\n"
    );
    assert_eq!(
        fs::read_to_string(out.path().join("sitemap.xml")).unwrap(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n  <url><loc>https://app.example.invalid/</loc></url>\n</urlset>\n"
    );
    // The picture is the pages' own, byte for byte: the app's site serves what its page names.
    assert_eq!(fs::read(out.path().join("social.jpg")).unwrap(), b"jpg");
}

#[test]
fn a_preview_names_the_real_address_of_the_app_in_its_page_and_the_build_is_made_the_same_every_time() {
    let root = repo();
    let one = root.path().join("one");
    let two = root.path().join("two");
    let dist = root.path().join("dist/web");
    app::assemble(root.path(), &dist, &one, &app_settings("https://app.example.invalid")).unwrap();
    app::assemble(root.path(), &dist, &two, &app_settings("https://app.example.invalid")).unwrap();
    for name in ["index.html", "robots.txt", "sitemap.xml", "social.jpg"] {
        assert_eq!(fs::read(one.join(name)).unwrap(), fs::read(two.join(name)).unwrap(), "{name}");
    }
    // Another address in the settings moves every address in the page, and nothing else in it.
    let moved = root.path().join("moved");
    app::assemble(root.path(), &dist, &moved, &app_settings("https://other.example.invalid")).unwrap();
    let page = fs::read_to_string(moved.join("index.html")).unwrap();
    assert!(page.contains("https://other.example.invalid/social.jpg") && !page.contains("app.example.invalid"), "{page}");
    app::check(&moved).unwrap();
}

#[test]
fn a_build_page_without_a_title_or_description_or_with_the_tags_already_is_refused() {
    let root = repo();
    let dist = root.path().join("dist/web");
    let assemble = |name: &str| app::assemble(root.path(), &dist, &root.path().join(name), &app_settings("https://app.example.invalid"));
    for (from, to, diagnostic) in [
        ("<title>DiaCraft: free online diagram editor</title>", "<title> </title>", "no title"),
        (
            "<meta name=\"description\" content=\"Draw diagrams in your browser, free &amp; with nothing to install.\">",
            "",
            "no <meta name=\"description\">",
        ),
        ("<meta charset=\"utf-8\">", "<meta charset=\"utf-8\"><link rel=\"canonical\" href=\"https://x.invalid/\">", "rel=\"canonical\" already"),
        ("<meta charset=\"utf-8\">", "<meta charset=\"utf-8\"><meta property=\"og:title\" content=\"x\">", "property=\"og: already"),
        ("<meta charset=\"utf-8\">", "<meta charset=\"utf-8\"><script type=\"application/ld+json\">{}</script>", "application/ld+json already"),
    ] {
        root.write("dist/web/index.html", TRUNK_PAGE.replace(from, to));
        let error = assemble(&format!("out-{}", from.len() + to.len())).unwrap_err();
        assert!(error.contains(diagnostic), "{from}: {error}");
    }
    // A build that has a file of the name of one the build makes is refused too.
    root.write("dist/web/index.html", TRUNK_PAGE);
    for name in ["robots.txt", "sitemap.xml", "social.jpg"] {
        root.write(&format!("dist/web/{name}"), "x");
        let error = assemble(&format!("clash-{name}")).unwrap_err();
        assert!(error.contains("already has a file of that name"), "{name}: {error}");
        fs::remove_file(root.path().join("dist/web").join(name)).unwrap();
    }
}

/// The check, after `change` is made to a finished folder.
fn refused(change: impl Fn(&Dir), diagnostic: &str) {
    let out = app_folder();
    change(&out);
    let error = app::check(out.path()).unwrap_err();
    assert!(error.contains(diagnostic), "expected {diagnostic:?} in {error:?}");
}

#[test]
fn the_check_holds_the_apps_page_its_data_and_the_files_beside_it() {
    let page = |change: &'static dyn Fn(String) -> String| move |o: &Dir| o.write("index.html", change(app_page(o)));
    refused(page(&|p| p.replace("<title>DiaCraft: free online diagram editor</title>", "<title></title>")), "no <title>");
    refused(page(&|p| p.replace("name=\"description\"", "name=\"keywords\"")), "no <meta name=\"description\">");
    refused(page(&|p| p.replace("<link rel=\"canonical\" href=\"https://app.example.invalid/\">", "")), "no canonical address");
    refused(
        page(&|p| {
            p.replace(
                "<link rel=\"canonical\" href=\"https://app.example.invalid/\">",
                "<link rel=\"canonical\" href=\"https://app.example.invalid/x\">",
            )
        }),
        "not the app's start page",
    );
    refused(
        page(&|p| {
            p.replace(
                "<link rel=\"canonical\" href=\"https://app.example.invalid/\">",
                "<link rel=\"canonical\" href=\"http://app.example.invalid/\">",
            )
        }),
        "not the app's start page",
    );
    refused(
        page(&|p| {
            p.replace("property=\"og:url\" content=\"https://app.example.invalid/\"", "property=\"og:url\" content=\"https://other.invalid/\"")
        }),
        "og:url",
    );
    refused(page(&|p| p.replace("og:title\" content=\"DiaCraft: free", "og:title\" content=\"Another: free")), "og:title");
    refused(
        page(&|p| p.replace("name=\"twitter:card\" content=\"summary_large_image\"", "name=\"twitter:card\" content=\"summary\"")),
        "twitter:card",
    );
    refused(
        page(&|p| {
            p.replace(
                "twitter:image\" content=\"https://app.example.invalid/social.jpg\"",
                "twitter:image\" content=\"https://app.example.invalid/gone.jpg\"",
            )
        }),
        "twitter:image",
    );
    refused(|o| fs::remove_file(o.path().join("social.jpg")).unwrap(), "has no social.jpg");
    refused(page(&|p| p.replace("\"@type\": \"WebApplication\"", "\"@type\": \"Product\"")), "must be a WebApplication");
    refused(page(&|p| p.replace("\"price\": \"0\"", "\"price\": \"5\"")), "free");
    refused(
        page(&|p| p.replace("\"browserRequirements\": \"Requires", "\"browserRequirements\": \" \",\n      \"x\": \"Requires")),
        "free, and say what it needs of a browser",
    );
    refused(page(&|p| p.replace("\"url\": \"https://app.example.invalid/\"", "\"url\": \"https://app.example.invalid/x\"")), "must be at");
    refused(page(&|p| p.replace("    {\n      \"@context\"", "    {\n      \"@context\" :")), "not JSON");
    refused(page(&|p| p.replace("</head>", "<script type=\"application/ld+json\">{}</script></head>")), "more than one block");
    refused(|o| o.write("robots.txt", "User-agent: *\nDisallow: /\n"), "robots.txt must let every crawler in");
    refused(|o| o.write("robots.txt", "User-agent: *\nAllow: /\n\nSitemap: https://other.invalid/sitemap.xml\n"), "robots.txt must");
    refused(
        |o| o.write("sitemap.xml", "<urlset><url><loc>https://app.example.invalid/x</loc></url></urlset>"),
        "sitemap.xml must hold the one address",
    );
    refused(
        |o| {
            o.write(
                "sitemap.xml",
                "<urlset><url><loc>https://app.example.invalid/</loc></url><url><loc>https://app.example.invalid/x</loc></url></urlset>",
            )
        },
        "sitemap.xml must hold the one address",
    );
    refused(|o| fs::remove_file(o.path().join("robots.txt")).unwrap(), "robots.txt");
    refused(|o| fs::remove_file(o.path().join("sitemap.xml")).unwrap(), "sitemap.xml");
}

#[test]
fn the_apps_page_is_read_as_text_with_its_tags_found_whatever_their_case_and_a_longer_name_is_not_a_tag() {
    let html = "<HTML><HEAD><TITLE> A &amp; B </TITLE><META NAME=\"description\" CONTENT=\"x &lt; y\"><metadata name=\"description\" content=\"no\"><meta\nname=\"theme-color\" content=\"#fff\"></HEAD></HTML>";
    assert_eq!(super::seo::app_page(html, &app_settings("https://a.example")).unwrap().matches("og:title\" content=\"A &amp; B\"").count(), 1);
    assert!(super::seo::check_app("<title>x</title>", "", "", &[]).is_err());
}

/// The app's page as Trunk writes it, as far as the tags go, with `tag` as the one that holds the description.
fn app_html_with(tag: &str) -> String {
    format!("<!DOCTYPE html><html><head><title>DiaCraft: a title</title>{tag}</head><body></body></html>")
}

/// What the check of the app's folder reads of `page`, made by `app_page`: `Ok` when the page, its robots file and its sitemap pass.
fn app_page_checked(page: &str) -> Result<(), String> {
    let settings = app_settings("https://app.example.invalid");
    seo::check_app(page, &seo::app_robots(&settings), &seo::app_sitemap(&settings), &[seo::APP_SOCIAL])
}

/// A `>` between the quotes of a value does not end the tag (as an HTML reader reads it): the description of `content="A > B"` is read whole, the page is made, and the check of what was made passes.
#[test]
fn a_greater_than_sign_inside_the_quotes_of_a_value_does_not_end_the_tag() {
    for (tag, as_written) in [
        (r#"<meta name="description" content="A > B">"#, "A &gt; B"),
        (r#"<meta name="description" content='A > B'>"#, "A &gt; B"),
        (r#"<meta name="description" content="it's > that">"#, "it's &gt; that"),
        (r#"<meta name='description' content='say "a > b"'>"#, "say &quot;a &gt; b&quot;"),
        (r#"<meta name = "description" content = "A > B" >"#, "A &gt; B"),
        (r#"<meta content="A > B" name="description">"#, "A &gt; B"),
        (r#"<meta name="description" data-note="1 > 0" content="A > B">"#, "A &gt; B"),
        ("<meta\nname=\"description\"\ncontent=\"A > B\"\n>", "A &gt; B"),
    ] {
        let page = seo::app_page(&app_html_with(tag), &app_settings("https://app.example.invalid")).unwrap_or_else(|e| panic!("{tag}: {e}"));
        for line in [
            format!("<meta property=\"og:description\" content=\"{as_written}\">"),
            format!("<meta name=\"twitter:description\" content=\"{as_written}\">"),
        ] {
            assert!(page.contains(&line), "{tag}: {line} is not in {page}");
        }
        assert!(page.contains(&format!("\"description\": \"{}\"", as_written.replace("&gt;", "\\u003e").replace("&quot;", "\\\""))), "{tag}: {page}");
        app_page_checked(&page).unwrap_or_else(|e| panic!("{tag}: {e}"));
    }
}

/// The tag is what its quotes say: a `<meta` inside a value is a part of the value, an unclosed quote ends the reading and no tag is found after it, and a quote that no `=` opens is a letter.
#[test]
fn a_tag_inside_a_quoted_value_is_no_tag_and_an_unclosed_quote_ends_the_reading() {
    let settings = app_settings("https://app.example.invalid");
    // A description that holds what looks like a tag: it is the value, whole.
    let inner = r##"<meta name="theme-color" data-x="<meta name=&quot;description&quot; content=&quot;no&quot;>" content="#fff"><meta name="description" content="yes">"##;
    let page = seo::app_page(&app_html_with(inner), &settings).unwrap();
    assert!(page.contains("<meta property=\"og:description\" content=\"yes\">"), "{page}");
    // The tag that the quote never closes, and the ones after it that its value would swallow, are not read: no description.
    for open in [r#"<meta name="description" content="A > B"#, r#"<meta name="description" content='A > B"#, r#"<meta name="description" content="A"#]
    {
        let error = seo::app_page(&app_html_with(&format!("{open}<meta name=description content=later>")), &settings).unwrap_err();
        assert!(error.contains("<meta name=\"description\">"), "{open}: {error}");
    }
    // A quote that is not the start of a value (no `=` before it) does not open one.
    let page = seo::app_page(&app_html_with(r#"<meta x"y name="description" content="A > B">"#), &settings).unwrap();
    assert!(page.contains("<meta property=\"og:description\" content=\"A &gt; B\">"), "{page}");
}

/// A page of two megabytes of open tags and unclosed quotes is read in one pass (a pass that looked for the end of each tag from its start would need minutes). The test is a guard against a hang,
/// not a stopwatch: it fails only if the work is not done within twenty seconds.
#[test]
fn a_page_of_tags_that_are_never_closed_is_read_in_one_pass() {
    let settings = app_settings("https://app.example.invalid");
    for unit in ["<meta a=\"", "<meta ", "<meta a='b' c=\"", "<link rel=\"canonical"] {
        let page = format!("<title>x</title>{}", unit.repeat((app::MAX_TEXT as usize - 64) / unit.len()));
        let (sender, receiver) = std::sync::mpsc::channel();
        let for_thread = settings.clone();
        std::thread::spawn(move || {
            let made = seo::app_page(&page, &for_thread);
            let checked = seo::check_app(&page, "", "", &[]);
            // `Err` only if the test has stopped waiting, which it has then failed.
            let _ = sender.send((made.is_err(), checked.is_err()));
        });
        let (made_failed, check_failed) =
            receiver.recv_timeout(std::time::Duration::from_secs(20)).unwrap_or_else(|_| panic!("{unit:?}: read in one pass"));
        assert!(made_failed && check_failed, "{unit:?}: a page with no description is refused");
    }
}

/// The app's page with `title` and `description` written as they are in the source (references and all), made by `app_page`.
fn app_page_of(title: &str, description: &str) -> Result<String, String> {
    let html =
        format!("<!DOCTYPE html><html><head><title>{title}</title><meta name=\"description\" content=\"{description}\"></head><body></body></html>");
    seo::app_page(&html, &app_settings("https://app.example.invalid"))
}

/// The references of a title and a description are the characters they stand for, in the social tags and in the data: `Tom &#38; Jerry` is `Tom & Jerry`, which the tags write as `Tom &amp; Jerry`
/// and the data as `Tom & Jerry`, and not `Tom &amp;#38; Jerry` in the tags and `&#38;` in the data.
#[test]
fn numeric_references_in_the_apps_title_and_description_are_decoded_as_the_named_ones_are() {
    // (as the source writes it, as a tag of the social cards writes it, as the data writes it)
    for (written, tag, data) in [
        ("Tom &#38; Jerry", "Tom &amp; Jerry", "Tom \\u0026 Jerry"),
        ("Tom &#x26; Jerry", "Tom &amp; Jerry", "Tom \\u0026 Jerry"),
        ("Tom &#X26; Jerry", "Tom &amp; Jerry", "Tom \\u0026 Jerry"),
        ("Tom &#0000038; Jerry", "Tom &amp; Jerry", "Tom \\u0026 Jerry"),
        ("Tom &amp; Jerry", "Tom &amp; Jerry", "Tom \\u0026 Jerry"),
        ("&#60;b&#62; &#x3c;i&#x3E;", "&lt;b&gt; &lt;i&gt;", "\\u003cb\\u003e \\u003ci\\u003e"),
        ("say &#34;hi&#34; &#x22;", "say &quot;hi&quot; &quot;", "say \\\"hi\\\" \\\""),
        ("it&#39;s &#x27;it&apos;", "it's 'it'", "it's 'it'"),
        ("caf&#233; &#x1F600; &#x10FFFF;", "café 😀 \u{10FFFF}", "café 😀 \u{10FFFF}"),
        // A reference is decoded once: what it stands for is not read as another.
        ("&#38;lt; &#38;#38;", "&amp;lt; &amp;#38;", "\\u0026lt; \\u0026#38;"),
        ("&amp;#38; &amp;lt;", "&amp;#38; &amp;lt;", "\\u0026#38; \\u0026lt;"),
        // A line break, a carriage return and a tab stay references in a tag (one line to a tag), and are the characters in the data.
        ("a&#10;b&#9;c&#13;d", "a&#10;b&#9;c&#13;d", "a\\nb\\tc\\rd"),
        // What is not a reference of ours stays as it is: a named reference that is not one of the five, a lone `&`, a `;` that no reference ends.
        ("&hellip; & R&D;", "&amp;hellip; &amp; R&amp;D;", "\\u0026hellip; \\u0026 R\\u0026D;"),
    ] {
        let page = app_page_of("T", written).unwrap_or_else(|e| panic!("{written}: {e}"));
        for line in
            [format!("<meta property=\"og:description\" content=\"{tag}\">"), format!("<meta name=\"twitter:description\" content=\"{tag}\">")]
        {
            assert!(page.contains(&line), "{written}: {line} is not in {page}");
        }
        assert!(page.contains(&format!("\"description\": \"{data}\"")), "{written}: {page}");
        app_page_checked(&page).unwrap_or_else(|e| panic!("{written}: {e}"));
        let page = app_page_of(written, "D").unwrap_or_else(|e| panic!("{written}: {e}"));
        for line in [format!("<meta property=\"og:title\" content=\"{tag}\">"), format!("<meta name=\"twitter:title\" content=\"{tag}\">")] {
            assert!(page.contains(&line), "{written}: {line} is not in {page}");
        }
        app_page_checked(&page).unwrap_or_else(|e| panic!("{written}: {e}"));
    }
}

/// A reference that is not a character is an error that names it, in a title and in a description, in the page that is made and in the one that is checked: the code point 0, a surrogate, a number past
/// U+10FFFF or past what a number of 32 bits holds, a control character that a browser reads as another (U+0080 to U+009F), a number with no digit or with a letter, a sign or a space in it, one that
/// the `;` does not close, and one that is longer than a reference may be.
#[test]
fn a_numeric_reference_that_is_not_a_character_is_an_error_in_the_title_and_in_the_description() {
    let long = format!("&#{}65;", "0".repeat(40));
    let bad = [
        "&#0;",
        "&#x0;",
        "&#00000000;",
        "&#xD800;",
        "&#xdfff;",
        "&#55296;",
        "&#57343;",
        "&#x110000;",
        "&#1114112;",
        "&#4294967296;",
        "&#99999999999999999999999;",
        "&#xFFFFFFFFFF;",
        "&#x80;",
        "&#128;",
        "&#x9F;",
        "&#159;",
        "&#;",
        "&#x;",
        "&#X;",
        "&#12a;",
        "&#xZ;",
        "&#+65;",
        "&#-65;",
        "&# 65;",
        "&#65 ;",
        "&#65",
        "&#65 and the text goes on",
        "&#x41",
        "&#",
        long.as_str(),
    ];
    for written in bad {
        let error = app_page_of("T", &format!("a {written} b")).unwrap_err();
        assert!(error.contains("description") && error.contains("character reference"), "{written}: {error}");
        let error = app_page_of(&format!("a {written} b"), "D").unwrap_err();
        assert!(error.contains("title") && error.contains("character reference"), "{written}: {error}");
        // In a tag of a page that was made, the check says so.
        let made = app_page_of("T", "D").unwrap();
        for (tag, key) in [("og:description", "property"), ("twitter:description", "name")] {
            let page = made.replace(&format!("<meta {key}=\"{tag}\" content=\"D\">"), &format!("<meta {key}=\"{tag}\" content=\"{written}\">"));
            assert_ne!(page, made);
            let error = app_page_checked(&page).unwrap_err();
            assert!(error.contains(tag) && error.contains("character reference"), "{written}: {error}");
        }
    }
    // The numbers just inside what is a character are not refused.
    for (written, text) in [
        ("&#1;", "\u{1}"),
        ("&#x7F;", "\u{7f}"),
        ("&#xA0;", "\u{a0}"),
        ("&#55295;", "\u{d7ff}"),
        ("&#xE000;", "\u{e000}"),
        ("&#1114111;", "\u{10ffff}"),
    ] {
        let page = app_page_of("T", written).unwrap_or_else(|e| panic!("{written}: {e}"));
        assert!(page.contains(&format!("<meta property=\"og:description\" content=\"{text}\">")), "{written}: {page}");
    }
}

/// A description of a million references is read in the time its size asks, and a million `&` with no reference is no more than text: each reference is looked for in a few bytes after its `&`.
#[test]
fn a_long_run_of_ampersands_is_read_in_one_pass_whatever_follows_them() {
    let many = "&#38;".repeat(200_000);
    let page = app_page_of("T", &many).unwrap();
    assert!(page.contains(&format!("content=\"{}\"", "&amp;".repeat(200_000))));
    let lone = "&".repeat(1_000_000);
    let page = app_page_of("T", &lone).unwrap();
    assert!(page.contains(&format!("content=\"{}\"", "&amp;".repeat(1_000_000))));
    // The first reference that is not one is the error, wherever it is: a million of them cost no more than one.
    assert!(app_page_of("T", &"&#".repeat(500_000)).is_err());
    assert!(app_page_of("T", &format!("{lone}&#{}", "9".repeat(1_000_000))).is_err());
}

/// `unescape` on its own: the five names (in lower case only), the numbers, text that holds more than one byte to a character, a reference read once, and the length a reference may have.
#[test]
fn the_references_of_an_attribute_are_decoded_once_and_a_reference_has_the_most_length_it_may() {
    for (text, plain) in [
        ("", ""),
        ("plain", "plain"),
        ("a &lt;&gt;&quot;&apos;&amp; b", "a <>\"'& b"),
        ("&", "&"),
        ("&&amp;", "&&"),
        ("&amp&amp;", "&amp&"),
        ("&AMP; &Lt; &amp ;", "&AMP; &Lt; &amp ;"),
        ("é&#38;é &#x672C;日本 &#X1f600;", "é&é 本日本 😀"),
        ("&#38;amp; &#x26;#38;", "&amp; &#38;"),
        ("x;&;y&#59;", "x;&;y;"),
    ] {
        assert_eq!(seo::unescape(text).unwrap(), plain, "{text:?}");
    }
    // A reference of the most bytes (32, with the zeros before its number) is read, and one byte more is not a reference that is closed.
    let longest = format!("&#{}65;", "0".repeat(29));
    assert_eq!(longest.len(), 1 + 32 + 1);
    assert_eq!(seo::unescape(&longest).unwrap(), "A");
    let too_long = format!("&#{}65;", "0".repeat(30));
    assert!(seo::unescape(&too_long).unwrap_err().contains("character reference"));
    // A reference that the text ends in the middle of is an error too, and a name that is long is only text.
    assert!(seo::unescape("&#").is_err() && seo::unescape("&#x4").is_err() && seo::unescape("&#x4;&#4").is_err());
    assert_eq!(seo::unescape(&format!("&{};", "a".repeat(100))).unwrap(), format!("&{};", "a".repeat(100)));
}

/// `code_point` for every number that matters: a character from 1 to U+10FFFF, but the control characters U+0080 to U+009F and the surrogates, and the character is the number.
#[test]
fn a_code_point_is_a_character_but_zero_the_surrogates_the_c1_controls_and_what_is_past_the_last() {
    for value in 0..0x12_0000u32 {
        let valid = value != 0 && !(0x80..=0x9f).contains(&value) && !(0xd800..=0xdfff).contains(&value) && value <= 0x10_ffff;
        assert_eq!(seo::code_point(value).map(u32::from), valid.then_some(value), "{value:#x}");
    }
    for value in [0x11_0000, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
        assert_eq!(seo::code_point(value), None, "{value:#x}");
    }
}
