//! The pages' download links against the release workflow. Each link of a page is the file of the release of the workspace's version, and each file is one that the release
//! workflow's last job expects in the release and lists in `SHA256SUMS.txt`: a link whose file the release never has would be a dead download.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::settings::{self, Settings};

/// Where the release workflow is, from the root of the repository.
pub(super) const WORKFLOW: &str = ".github/workflows/release.yml";

/// The most the workflow's text may be.
const MAX_WORKFLOW: u64 = 512 * 1024;

/// The files the release workflow's checksums job expects in the release, as patterns with `{version}` for the version (the tag without its `v`): each `"diacraft-$VERSION-..."` word of its
/// `expected=$(printf ... | sort` command. **The MCP bundles are not in it**: their words start `"diacraft-mcp-$VERSION-..."`, they are files of the release and no download of the
/// site (the site shows no button for one), and `xtask::mcp::BUNDLES` and a test of the workflow hold them to the same list.
pub(super) fn listed(workflow: &str) -> Result<Vec<String>, String> {
    let start =
        workflow.find("expected=$(printf").ok_or("the checksums job of the release workflow has no list of the files (`expected=$(printf`)")?;
    let list = workflow.get(start..).unwrap_or("");
    let end = list.find("| sort").ok_or("the list of the files in the release workflow is not sorted (`| sort`)")?;
    let names: Vec<String> = list
        .get(..end)
        .unwrap_or("")
        .split('"')
        .filter(|word| word.starts_with("diacraft-$VERSION-"))
        .map(|word| word.replace("$VERSION", "{version}"))
        .collect();
    if names.is_empty() {
        return Err("the checksums job of the release workflow lists no file (`\"diacraft-$VERSION-...\"`)".into());
    }
    Ok(names)
}

/// The names of the files in the release of the pages' own version, from the release workflow of the repository at `root`.
fn files(root: &Path, settings: &Settings) -> Result<BTreeSet<String>, String> {
    let path = root.join(WORKFLOW);
    let meta = fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.len() > MAX_WORKFLOW {
        return Err(format!("{}: expected a file of at most {MAX_WORKFLOW} bytes", path.display()));
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(listed(&text).map_err(|e| format!("{WORKFLOW}: {e}"))?.iter().map(|pattern| settings.file_name(pattern)).collect())
}

/// What the pages in `dir` (the folder of the pages, in the repository at `root`) link: every asset name of `site.json` is a file the release workflow lists, and every download link of every
/// page is an address in the folder of the release of the workspace's version, naming one of those files. A link with no file name yet (a draft of `site.json`) is not held to it.
pub(super) fn check(root: &Path, dir: &Path, settings: &Settings, pages: &[String]) -> Result<(), String> {
    let files = files(root, settings)?;
    for (id, pattern) in &settings.assets {
        if !pattern.is_empty() && !files.contains(&settings.file_name(pattern)) {
            return Err(format!(
                "site.json: assets.{id} is {pattern}, which the release workflow does not list: the release has none of that name (the list of `expected=$(printf` in {WORKFLOW})"
            ));
        }
    }
    let folder = settings::download_folder(settings);
    for page in pages {
        let path = dir.join(page);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // An anchor is `<a` and a space, and its first attribute is read as the others are: after a space (`<article` and `<abbr` are no anchors).
        for piece in text.split("<a").skip(1).filter(|piece| piece.starts_with(char::is_whitespace)) {
            let tag = piece.split('>').next().unwrap_or("");
            let Some(download) = super::app::attribute(tag, "data-download") else { continue };
            let asset = settings.asset(download).ok_or_else(|| format!("{page}: {download} is not one of the downloads"))?;
            if asset.is_empty() {
                continue;
            }
            let href = super::app::attribute(tag, "href").ok_or_else(|| format!("{page}: the link of {download} has no href"))?;
            let name = href.strip_prefix(&folder).filter(|name| !name.is_empty() && !name.contains(['/', '?', '#'])).ok_or_else(|| {
                format!("{page}: the link of {download} is {href}, and not a file of the release v{}: {folder}<file>", settings.version)
            })?;
            let name = super::links::percent_decode(name).map_err(|e| format!("{page}: {href}: {e}"))?;
            if !files.contains(&name) {
                return Err(format!("{page}: the link of {download} is the file {name}, which the release workflow does not list ({WORKFLOW})"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check, listed};
    use crate::site::settings::{DOWNLOADS, Settings, direct_address};
    use std::fs;
    use std::path::PathBuf;

    /// The checksums job of a workflow, as the repository's has it in short.
    const WORKFLOW: &str = r#"
  checksums:
    steps:
      - run: |
          expected=$(printf '%s\n' \
            "diacraft-$VERSION-linux-x86_64.tar.gz" "diacraft-$VERSION-linux-aarch64.tar.gz" \
            "diacraft-$VERSION-macos-arm64.dmg" "diacraft-$VERSION-macos-x86_64.dmg" \
            "diacraft-mcp-$VERSION-linux-x86_64.mcpb" | sort | tr '\n' ' ' | sed 's/ $//')
          found=$(cd downloads && ls | sort)
"#;

    struct Root(PathBuf);

    impl Root {
        fn new(workflow: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!("diacraft-site-downloads-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(path.join(".github/workflows")).unwrap();
            fs::create_dir_all(path.join("site")).unwrap();
            fs::write(path.join(".github/workflows/release.yml"), workflow).unwrap();
            Root(path)
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    /// Settings whose four downloads are the four files of [`WORKFLOW`], and the other eight have no name yet.
    fn settings() -> Settings {
        let mut assets: std::collections::BTreeMap<String, String> = DOWNLOADS.iter().map(|id| ((*id).to_owned(), String::new())).collect();
        for (id, name) in [
            ("linux-x86_64-targz", "diacraft-{version}-linux-x86_64.tar.gz"),
            ("linux-arm64-targz", "diacraft-{version}-linux-aarch64.tar.gz"),
            ("macos-arm64-dmg", "diacraft-{version}-macos-arm64.dmg"),
            ("macos-x86_64-dmg", "diacraft-{version}-macos-x86_64.dmg"),
        ] {
            assets.insert(id.to_owned(), name.to_owned());
        }
        Settings {
            domain: "https://d.example".into(),
            app_url: "https://a.example".into(),
            repository: "owner/drawing".into(),
            assets,
            version: "0.2.0".into(),
            licences: Vec::new(),
            comparison: None,
        }
    }

    fn link(settings: &Settings, id: &str) -> String {
        let asset = settings.asset(id).unwrap();
        format!(r#"<a data-repo="/releases/download" href="{}" data-download="{id}" data-asset="{asset}">x</a>"#, direct_address(settings, asset))
    }

    fn pages(root: &Root, body: &str) -> Vec<String> {
        fs::write(root.0.join("site/index.html"), format!("<html><body>{body}</body></html>")).unwrap();
        vec!["index.html".to_owned()]
    }

    #[test]
    fn the_list_is_the_words_of_the_checksums_job_with_the_version_left_open() {
        assert_eq!(
            listed(WORKFLOW).unwrap(),
            [
                "diacraft-{version}-linux-x86_64.tar.gz",
                "diacraft-{version}-linux-aarch64.tar.gz",
                "diacraft-{version}-macos-arm64.dmg",
                "diacraft-{version}-macos-x86_64.dmg"
            ]
        );
        // An MCP bundle of the list is a file of the release and no download of the site: the four are all the list gives.
        assert!(!listed(WORKFLOW).unwrap().iter().any(|name| name.contains("mcp")));
        // A word named by the tag (`$NAME`, which the file names were made of before a change) is no file of the list.
        for bad in [
            "",
            "expected=$(printf '%s\\n' \"diacraft-$VERSION-a\"",
            "expected=$(printf '%s\\n' \"a\" \"b\" | sort",
            "expected=$(printf | sort",
            "expected=$(printf '%s\\n' \"diacraft-$NAME-a\" | sort",
        ] {
            assert!(listed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_link_is_a_file_of_the_release_of_the_version_that_the_workflow_lists() {
        let root = Root::new(WORKFLOW);
        let settings = settings();
        let body: String =
            ["linux-x86_64-targz", "linux-arm64-targz", "macos-arm64-dmg", "macos-x86_64-dmg"].iter().map(|id| link(&settings, id)).collect();
        let pages = pages(&root, &body);
        check(&root.0, &root.0.join("site"), &settings, &pages).unwrap();
    }

    #[test]
    fn an_asset_the_workflow_does_not_list_is_refused_where_site_json_names_it() {
        let root = Root::new(WORKFLOW);
        let mut settings = settings();
        settings.assets.insert("linux-arm64-deb".into(), "diacraft-{version}-linux-aarch64.deb".into());
        let error = check(&root.0, &root.0.join("site"), &settings, &pages(&root, "")).unwrap_err();
        assert!(error.contains("assets.linux-arm64-deb") && error.contains("does not list"), "{error}");
        // A name made with the tag, which has the `v`, is another file than the one made with the version; so is the CPU in the words the names had before.
        settings.assets.insert("linux-arm64-deb".into(), String::new());
        settings.assets.insert("linux-arm64-targz".into(), "diacraft-{tag}-linux-aarch64.tar.gz".into());
        let error = check(&root.0, &root.0.join("site"), &settings, &pages(&root, "")).unwrap_err();
        assert!(error.contains("assets.linux-arm64-targz"), "{error}");
        settings.assets.insert("linux-arm64-targz".into(), "diacraft-{version}-linux-arm64.tar.gz".into());
        let error = check(&root.0, &root.0.join("site"), &settings, &pages(&root, "")).unwrap_err();
        assert!(error.contains("assets.linux-arm64-targz"), "{error}");
    }

    #[test]
    fn a_link_that_is_not_a_file_of_the_release_of_the_version_is_refused() {
        let root = Root::new(WORKFLOW);
        let settings = settings();
        let good = link(&settings, "linux-arm64-targz");
        for (bad, said) in [
            // The page of the latest release, which was the link before a change.
            (
                good.replace(
                    &direct_address(&settings, "diacraft-{version}-linux-aarch64.tar.gz"),
                    "https://github.com/owner/drawing/releases/latest",
                ),
                "not a file of the release v0.2.0",
            ),
            // A file of an older release.
            (good.replace("/v0.2.0/", "/v0.1.0/"), "not a file of the release v0.2.0"),
            // Another repository.
            (good.replace("owner/drawing", "other/drawing"), "not a file of the release v0.2.0"),
            // A file the workflow does not list.
            (good.replace("linux-aarch64.tar.gz", "linux-aarch64.tar.xz"), "does not list"),
            // The name the file had before a change, with the tag's `v` and the CPU as the site says it: the release has no such file.
            (good.replace("diacraft-0.2.0-linux-aarch64.tar.gz", "diacraft-v0.2.0-linux-arm64.tar.gz"), "does not list"),
            // A file in a folder under the release's, a query and a fragment.
            (good.replace("/diacraft-0.2.0-linux", "/x/diacraft-0.2.0-linux"), "not a file of the release v0.2.0"),
            (good.replace(".tar.gz\"", ".tar.gz?raw=1\""), "not a file of the release v0.2.0"),
            (good.replace(".tar.gz\"", ".tar.gz#x\""), "not a file of the release v0.2.0"),
            // No file name at all.
            (good.replace("diacraft-0.2.0-linux-aarch64.tar.gz", ""), "not a file of the release v0.2.0"),
        ] {
            let error = check(&root.0, &root.0.join("site"), &settings, &pages(&root, &bad)).unwrap_err();
            assert!(error.contains(said), "{bad}: {error}");
        }
        // A link with no href is an error and not a panic.
        let error = check(&root.0, &root.0.join("site"), &settings, &pages(&root, r#"<a data-download="linux-arm64-targz">x</a>"#)).unwrap_err();
        assert!(error.contains("no href"), "{error}");
        // A link that names no download of the list.
        let error =
            check(&root.0, &root.0.join("site"), &settings, &pages(&root, r#"<a data-download="linux-riscv-tgz" href="x">x</a>"#)).unwrap_err();
        assert!(error.contains("not one of the downloads"), "{error}");
    }

    #[test]
    fn a_draft_link_with_no_file_name_is_not_held_to_the_list() {
        let root = Root::new(WORKFLOW);
        let settings = settings();
        let draft = link(&settings, "linux-x86_64-deb");
        assert!(draft.contains("/releases/tag/v0.2.0"), "{draft}");
        check(&root.0, &root.0.join("site"), &settings, &pages(&root, &draft)).unwrap();
    }

    #[test]
    fn a_missing_or_listless_workflow_is_an_error_naming_it() {
        let root = Root::new("name: Release\n");
        let error = check(&root.0, &root.0.join("site"), &settings(), &pages(&root, "")).unwrap_err();
        assert!(error.contains("release.yml") && error.contains("no list of the files"), "{error}");
        fs::remove_file(root.0.join(".github/workflows/release.yml")).unwrap();
        let error = check(&root.0, &root.0.join("site"), &settings(), &pages(&root, "")).unwrap_err();
        assert!(error.contains("release.yml"), "{error}");
    }
}
