//! The deploy of the two websites to Cloudflare Pages: the release workflow's `deploy` job, which waits for every build, downloads the two sites the `site` job made and
//! sends each to its Pages project with one tool (`wrangler`, installed from a lock), and the places that name that tool's version. Nothing here installs or runs the tool or reaches
//! Cloudflare; these are the names and numbers that several files each hold.

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The text of one job of the release workflow: from its `  <name>:` line to the line of the next job, with the comment lines left out.
fn job(name: &str) -> String {
    let workflow = read(".github/workflows/release.yml");
    let mut in_job = false;
    let mut lines = Vec::new();
    for line in workflow.lines() {
        let is_job_line = line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') && !line.trim_start().starts_with('#');
        if is_job_line {
            in_job = line.trim() == format!("{name}:");
        }
        if in_job && !line.trim_start().starts_with('#') {
            lines.push(line);
        }
    }
    assert!(!lines.is_empty(), "release.yml has no `{name}` job");
    lines.join("\n")
}

/// The lines of a job before its steps: its `needs`, `if`, `permissions` and `env`.
fn header(name: &str) -> String {
    let text = job(name);
    text.split("\n    steps:\n").next().unwrap_or_default().to_owned()
}

/// The steps of a job as (the name, the text of the step), in order. A step starts at a line of six spaces and `- `; a step that only uses an action is named by that line.
fn steps(job_name: &str) -> Vec<(String, String)> {
    let text = job(job_name);
    let steps_at = text.find("\n    steps:\n").unwrap_or_else(|| panic!("the {job_name} job has steps"));
    let mut steps: Vec<(String, String)> = Vec::new();
    for line in text[steps_at..].lines().skip(2) {
        if let Some(rest) = line.strip_prefix("      - ") {
            let name = rest.strip_prefix("name: ").map(str::to_owned).unwrap_or_else(|| rest.to_owned());
            steps.push((name, String::new()));
        }
        if let Some((_, text)) = steps.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    steps
}

fn step(job_name: &str, name: &str) -> String {
    let all = steps(job_name);
    let found: Vec<&(String, String)> = all.iter().filter(|(n, _)| n == name).collect();
    assert_eq!(found.len(), 1, "the {job_name} job has one step `{name}`, it has {:?}", all.iter().map(|(n, _)| n).collect::<Vec<_>>());
    found[0].1.clone()
}

fn position(job_name: &str, name: &str) -> usize {
    steps(job_name).iter().position(|(n, _)| n == name).unwrap_or_else(|| panic!("the {job_name} job has no step `{name}`"))
}

/// The value of a `key: value` line of a text (the first such line), without quotes.
fn value(text: &str, key: &str) -> String {
    let prefix = format!("{key}: ");
    let line = text.lines().map(str::trim).find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("no `{key}:` line in\n{text}"));
    line[prefix.len()..].trim().trim_matches('"').to_owned()
}

/// The condition of the job that deploys: a pushed tag (`publish`; a run by hand on a branch has none) that is not a pre-release (`v0.2.0-rc1` is the rehearsal that
/// `docs/releasing.md` offers, and it must not replace the live sites).
const FOR_A_RELEASE_TAG: &str = "needs.plan.outputs.publish == 'true' && needs.plan.outputs.prerelease == 'false'";

const CHECKOUT: &str = "Check out the repository, which holds the lock of wrangler";
const PAGES: &str = "Deploy the pages to Cloudflare Pages";
const APP: &str = "Deploy the web app to Cloudflare Pages";
const INSTALL: &str = "Install wrangler, the Cloudflare Pages tool";
const GET_PAGES: &str = "Download the pages the site job made";
const GET_APP: &str = "Download the web app the site job made";

/// The owner's ruling (2026-11-24): the tag deploys diacraft.dev (the pages) and diacraft.app (the web app) together with the downloads. Each folder goes to the project of
/// its own site, on the production branch the projects have, and a run by hand on a branch or a pre-release tag deploys nothing.
#[test]
fn a_pushed_release_tag_deploys_the_pages_to_diacraft_site_and_the_app_to_diacraft_app_and_nothing_else_does() {
    assert_eq!(value(&header("deploy"), "if"), FOR_A_RELEASE_TAG, "the deploy job runs only for a pushed tag that is not a pre-release");
    for (name, folder, project) in [(PAGES, "site-dist", "diacraft-site"), (APP, "app-dist", "diacraft-app")] {
        let run = value(&step("deploy", name), "run");
        assert!(run.starts_with(&format!("wrangler pages deploy {folder} --project-name {project} --branch main ")), "{name}: {run}");
        // The deployment is named for the commit and the tag, not for what the checkout's git says of a detached head.
        assert!(run.contains("--commit-hash \"$GITHUB_SHA\"") && run.contains("--commit-message \"DiaCraft $NAME\""), "{name}: {run}");
    }
    // One project for each site, and no folder goes to both.
    let project = |name: &str| value(&step("deploy", name), "run").split("--project-name ").nth(1).map(str::to_owned);
    assert_ne!(project(PAGES), project(APP));
    assert_eq!(value(&header("deploy"), "NAME"), "${{ needs.plan.outputs.name }}", "the deployment's message names the tag");
}

/// The owner's ruling (2026-11-24): the deploy waits for every build. The job needs the plan, the site, the three builds and the checksums, and it has no status function in its
/// condition, so GitHub's own `success()` applies: one job that failed, was cancelled or was skipped leaves the sites as they are.
#[test]
fn the_deploy_job_waits_for_the_site_the_three_builds_and_the_checksums_and_runs_only_when_they_all_succeed() {
    let needs = value(&header("deploy"), "needs");
    let mut needed: Vec<&str> = needs.trim_matches(|c| c == '[' || c == ']').split(',').map(str::trim).collect();
    needed.sort_unstable();
    assert_eq!(needed, ["checksums", "linux", "macos", "plan", "site", "windows"], "the deploy job's needs");
    for name in &needed {
        job(name); // each is a job of the workflow
    }
    let condition = value(&header("deploy"), "if");
    for status in ["always()", "failure()", "cancelled()", "success()"] {
        assert!(!condition.contains(status), "`{status}` in the condition changes when the job runs: {condition}");
    }
    // The site job only builds: nothing of the deploy is left in it.
    let site = job("site");
    assert!(
        !site.contains("wrangler") && !site.contains("CLOUDFLARE") && !site.contains("pages deploy"),
        "the site job deploys nothing: the deploy job does"
    );
}

/// The deploy sends what the site job built and checked: the job takes the two folders from the artifacts that job kept (the same names, and the folders the deploys name), then installs
/// the tool, then deploys. The artifacts are the ones of this run: the name holds the tag. Hidden files travel too, so that the folder deployed is the folder built.
#[test]
fn the_deploy_job_downloads_the_two_artifacts_the_site_job_kept_installs_wrangler_and_then_deploys() {
    let order = [CHECKOUT, GET_PAGES, GET_APP, INSTALL, PAGES, APP];
    let at: Vec<usize> = order.iter().map(|name| position("deploy", name)).collect();
    assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "the steps are in the wrong order: {order:?} at {at:?}");
    for (get, keep, folder, prefix) in [
        (GET_PAGES, "Keep the pages as an artifact", "site-dist", "diacraft-site-"),
        (GET_APP, "Keep the web app as an artifact", "app-dist", "diacraft-app-"),
    ] {
        let kept = step("site", keep);
        let got = step("deploy", get);
        let name = format!("{prefix}${{{{ needs.plan.outputs.name }}}}");
        assert_eq!(value(&kept, "name"), name, "{keep}");
        assert_eq!(value(&got, "name"), name, "{get}: the site job's artifact");
        assert_eq!(value(&kept, "path"), folder, "{keep}");
        assert_eq!(value(&got, "path"), folder, "{get}: the folder the deploy names");
        assert!(got.contains("uses: actions/download-artifact@"), "{get}: GitHub's own action");
        assert_eq!(value(&kept, "include-hidden-files"), "true", "{keep}");
    }
}

/// The token may edit every Pages project of the account, so only the two steps that deploy read the secrets (not the install of a tool from the npm registry, nor any other
/// step or job), and the tool's usage reports are off in the whole job.
#[test]
fn only_the_two_deploy_steps_read_the_cloudflare_secrets_and_the_tools_reports_are_off() {
    let workflow = read(".github/workflows/release.yml");
    for secret in ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"] {
        let lines: Vec<&str> = workflow.lines().filter(|l| !l.trim_start().starts_with('#') && l.contains(secret)).collect();
        assert_eq!(lines.len(), 2, "{secret} is read by the two deploy steps and by nothing else: {lines:?}");
    }
    assert_eq!(workflow.lines().filter(|l| !l.trim_start().starts_with('#') && l.contains("secrets.CLOUDFLARE_")).count(), 4);
    for name in [PAGES, APP] {
        let text = step("deploy", name);
        assert!(text.contains("CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}"), "{name}");
        assert!(text.contains("CLOUDFLARE_ACCOUNT_ID: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}"), "{name}");
    }
    assert!(!header("deploy").contains("secrets."), "the secrets are not in the job's own env, where every step would see them");
    for (name, text) in steps("deploy") {
        assert_eq!(text.contains("secrets."), name == PAGES || name == APP, "{name}");
    }
    assert_eq!(value(&header("deploy"), "WRANGLER_SEND_METRICS"), "false", "wrangler sends a usage report to Cloudflare unless it is told not to");
    // The job cannot write to the repository or its releases.
    assert_eq!(value(&header("deploy"), "contents"), "read");
    // A tool is run from the lock's install, never fetched by `npx` at the time of a deploy.
    let live = workflow.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    assert!(!live.contains("npx") && !live.contains("wrangler@"), "wrangler is installed from the lock, at one version");
}

/// The install is the lock's (`npm ci`: each of the packages by its SHA-512, none changed or added), with no install script run and Node held to the version wrangler asks for
/// (`engines` of its package), and the program that comes out reports the version asked for.
#[test]
fn wrangler_is_installed_from_the_lock_with_no_install_script_and_must_report_the_version_asked_for() {
    let text = step("deploy", INSTALL);
    let run = text.split("run: |").nth(1).unwrap_or_else(|| panic!("the install step is a script: {text}"));
    for needed in [
        "npm ci ",
        "--prefix packaging/cloudflare",
        "--ignore-scripts",
        "--engine-strict",
        "/wrangler\" --version",
        "$WRANGLER_VERSION",
        "$GITHUB_PATH",
    ] {
        assert!(run.contains(needed), "the install step does not say `{needed}`:\n{run}");
    }
    assert!(!run.contains("npm install") && !run.contains("npx"), "{run}");
    assert!(
        !run.contains("--omit=optional") && !run.contains("--no-optional"),
        "wrangler's own binaries are optional dependencies of it, and it stops at once without them"
    );
}

/// The version of wrangler is written in four places: the manifest (`packaging/cloudflare/package.json`), its lock, `WRANGLER_VERSION` of the workflow (what the install checks the
/// program against) and the table of pinned tools in `docs/releasing.md`, which also gives the SHA-512 of the package. One exact version in each, the same.
#[test]
fn the_wrangler_version_is_one_exact_version_in_the_manifest_the_lock_the_workflow_and_the_releasing_doc() {
    let manifest: serde_json::Value = serde_json::from_str(&read("packaging/cloudflare/package.json")).unwrap();
    let dependencies = manifest["dependencies"].as_object().expect("the manifest has dependencies");
    assert_eq!(dependencies.len(), 1, "wrangler is the one tool: {dependencies:?}");
    let version = dependencies["wrangler"].as_str().expect("wrangler has a version");
    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "`{version}` is not an exact version (no range, no tag)"
    );
    assert!(parts[0] == "4", "`pages deploy --branch` is wrangler 4's: {version}");

    let lock: serde_json::Value = serde_json::from_str(&read("packaging/cloudflare/package-lock.json")).unwrap();
    assert_eq!(lock["lockfileVersion"], 3);
    assert_eq!(
        lock["packages"][""]["dependencies"]["wrangler"].as_str(),
        Some(version),
        "the lock was made for another version: `npm install --package-lock-only --ignore-scripts`"
    );
    let locked = &lock["packages"]["node_modules/wrangler"];
    assert_eq!(locked["version"].as_str(), Some(version));
    let integrity = locked["integrity"].as_str().expect("wrangler has an integrity in the lock");

    assert_eq!(value(&header("deploy"), "WRANGLER_VERSION"), version, "the workflow checks the program against another version");
    let doc = read("docs/releasing.md");
    let row = doc
        .lines()
        .find(|l| l.starts_with("| wrangler |"))
        .unwrap_or_else(|| panic!("docs/releasing.md has no row for wrangler in its table of pinned tools"));
    assert!(
        row.contains(&format!("| {version} |")) && row.contains(integrity),
        "the wrangler row of docs/releasing.md says another version or integrity than the lock: {row}"
    );
}

/// Every package of the lock is one file of the npm registry, held to a SHA-512: no package from a git address, a folder or another host, and none without a checksum (`npm ci`
/// refuses a package whose bytes differ, and takes none that is not in the lock).
#[test]
fn every_package_of_the_wrangler_lock_comes_from_the_npm_registry_and_has_a_sha512_checksum() {
    let lock: serde_json::Value = serde_json::from_str(&read("packaging/cloudflare/package-lock.json")).unwrap();
    let packages = lock["packages"].as_object().expect("the lock has packages");
    assert!(packages.len() > 20, "the lock holds wrangler and what it needs: {}", packages.len());
    for (path, package) in packages {
        if path.is_empty() {
            continue; // the manifest itself
        }
        let resolved = package["resolved"].as_str().unwrap_or_else(|| panic!("{path} has no resolved address"));
        assert!(resolved.starts_with("https://registry.npmjs.org/") && resolved.ends_with(".tgz"), "{path}: {resolved}");
        let integrity = package["integrity"].as_str().unwrap_or_else(|| panic!("{path} has no integrity"));
        // sha512 in base64 is 64 bytes: 86 characters and two of padding.
        let digest = integrity.strip_prefix("sha512-").unwrap_or_else(|| panic!("{path}: {integrity}"));
        assert!(
            digest.len() == 88 && digest.ends_with("==") && digest.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
            "{path}: {integrity}"
        );
        assert!(package.get("link").is_none(), "{path} is a link to a folder");
    }
}

/// The owner's ruling (2026-11-24): wrangler's LGPL dependency (the image library under `sharp`) is allowed because the tool is a CI-only deploy tool that is never shipped, and the rules
/// say so by name, in the private file and in the public one (the published tree has the public file as `AGENTS.md`, and only that one).
#[test]
fn the_rules_name_the_lgpl_exception_for_the_deploy_tool_and_the_tool_is_the_only_npm_package_of_the_repository() {
    let mut read_any = false;
    for rel in ["AGENTS.md", "public/AGENTS.md"] {
        if !xtask::root().join(rel).exists() {
            continue;
        }
        read_any = true;
        let rules = read(rel);
        for needed in ["One named exception", "a CI-only deploy tool, never shipped", "`wrangler`", "packaging/cloudflare/package-lock.json", "LGPL"]
        {
            assert!(rules.contains(needed), "{rel} does not say `{needed}`");
        }
    }
    assert!(read_any, "the rules file is there");
    // The exception covers the one tool: no other manifest of npm packages is in the repository.
    let mut manifests: Vec<String> =
        xtask::repo_files(&xtask::root()).unwrap().into_iter().filter(|f| f.ends_with("package.json") || f.ends_with("package-lock.json")).collect();
    manifests.sort();
    assert_eq!(
        manifests,
        ["packaging/cloudflare/package-lock.json", "packaging/cloudflare/package.json"],
        "an npm package outside the deploy tool needs its own ruling"
    );
}
