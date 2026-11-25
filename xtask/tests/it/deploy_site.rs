//! The website deployed by hand (the owner, 2026-11-25): `.github/workflows/deploy-site.yml` builds the two sites as the release workflow's `site` job does, and deploys them as its
//! `deploy` job does, for a fix of the pages or the app between releases; and `packaging/install-site-tools.sh`, the one script that installs the tools that build the app for both workflows.
//! Nothing here runs a workflow or reaches Cloudflare; these are the names, the numbers and the rules that the two workflows each hold.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::TempDir;

const BY_HAND: &str = ".github/workflows/deploy-site.yml";
const RELEASE: &str = ".github/workflows/release.yml";

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The text of one job of a workflow, without its comment lines: from its `  <name>:` line to the line of the next job.
fn job(workflow: &str, name: &str) -> String {
    let mut in_job = false;
    let mut lines = Vec::new();
    for line in read(workflow).lines() {
        let is_job_line = line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') && !line.trim_start().starts_with('#');
        if is_job_line {
            in_job = line.trim() == format!("{name}:");
        }
        if in_job && !line.trim_start().starts_with('#') {
            lines.push(line.to_owned());
        }
    }
    assert!(!lines.is_empty(), "{workflow} has no `{name}` job");
    lines.join("\n")
}

/// The lines of a job before its steps.
fn header(workflow: &str, name: &str) -> String {
    job(workflow, name).split("\n    steps:\n").next().unwrap_or_default().to_owned()
}

/// The steps of a job as (the name, the text of the step), in order; a step that only uses an action is named by that line.
fn steps(workflow: &str, job_name: &str) -> Vec<(String, String)> {
    let text = job(workflow, job_name);
    let at = text.find("\n    steps:\n").unwrap_or_else(|| panic!("{workflow}: the {job_name} job has steps"));
    let mut steps: Vec<(String, String)> = Vec::new();
    for line in text[at..].lines().skip(2) {
        if let Some(rest) = line.strip_prefix("      - ") {
            steps.push((rest.strip_prefix("name: ").map(str::to_owned).unwrap_or_else(|| rest.to_owned()), String::new()));
        }
        if let Some((_, text)) = steps.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    steps
}

fn step(workflow: &str, job_name: &str, name: &str) -> String {
    let all = steps(workflow, job_name);
    let found: Vec<&(String, String)> = all.iter().filter(|(n, _)| n == name).collect();
    assert_eq!(found.len(), 1, "{workflow}: the {job_name} job has one step `{name}`, it has {:?}", all.iter().map(|(n, _)| n).collect::<Vec<_>>());
    found[0].1.clone()
}

/// The value of the first `key: value` line of a text, without quotes.
fn value(text: &str, key: &str) -> String {
    let prefix = format!("{key}: ");
    let line = text.lines().map(str::trim).find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("no `{key}:` line in\n{text}"));
    line[prefix.len()..].trim().trim_matches('"').to_owned()
}

/// The lines of a workflow that are not comments.
fn live(workflow: &str) -> Vec<String> {
    read(workflow).lines().filter(|l| !l.trim_start().starts_with('#')).map(str::to_owned).collect()
}

/// The owner's ruling (2026-11-25): a site or app fix between releases is deployed by hand, from `main` only, with a token that reads the repository and nothing else.
#[test]
fn the_website_is_deployed_by_hand_from_main_only_by_a_workflow_that_reads_the_repository_and_nothing_else() {
    let lines = live(BY_HAND);
    // The one trigger is a run by hand.
    let on = lines.iter().position(|l| l == "on:").expect("an `on:` block");
    let triggers: Vec<&str> = lines[on + 1..].iter().take_while(|l| l.starts_with("  ")).map(|l| l.trim()).collect();
    assert_eq!(triggers, ["workflow_dispatch:"], "the workflow starts by hand and in no other way");
    // The token of the workflow, and of each job, can read the repository and nothing more.
    let text = lines.join("\n");
    assert!(text.contains("\npermissions:\n  contents: read\n"), "the workflow's permissions are contents: read");
    for name in ["site", "deploy"] {
        let header = header(BY_HAND, name);
        assert!(header.contains("permissions:\n      contents: read"), "{name}: {header}");
        assert!(!header.contains("write"), "{name}: {header}");
    }
    assert!(!text.contains("contents: write") && !text.contains("id-token"), "no write permission anywhere");
    // Main only: the first step of the build stops any other branch, before the repository is even checked out, and the deploy job has the condition too.
    let first = &steps(BY_HAND, "site")[0];
    assert_eq!(first.0, "Stop unless this is main");
    assert!(first.1.contains("REF: ${{ github.ref }}") && first.1.contains("refs/heads/main") && first.1.contains("exit 1"), "{}", first.1);
    let script = first.1.split("run: |").nth(1).expect("the step is a script");
    assert!(!script.contains("${{"), "the ref is read through the environment, and not written into the text of the script: {script}");
    assert_eq!(value(&header(BY_HAND, "deploy"), "if"), "github.ref == 'refs/heads/main'");
    assert_eq!(value(&header(BY_HAND, "deploy"), "needs"), "[site]", "nothing is deployed unless the build passed");
}

/// The build is the release workflow's `site` job: the same commands, from one script for the tools, with the same versions and checksums; and it makes no download.
#[test]
fn it_builds_the_two_sites_as_the_release_workflows_site_job_does_and_never_a_download() {
    let by_hand = steps(BY_HAND, "site");
    let release = steps(RELEASE, "site");
    for (name, command) in [
        ("Install trunk, wasm-bindgen and wasm-opt", "bash packaging/install-site-tools.sh"),
        ("Check the pages", "cargo xtask site"),
        ("Build the web app and make the two sites", "cargo xtask site --web --out site-dist --app-out app-dist"),
    ] {
        assert_eq!(value(&by_hand.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("by hand: no `{name}`")).1, "run"), command, "{name}");
        assert_eq!(value(&release.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("release: no `{name}`")).1, "run"), command, "{name}");
    }
    // The tools are the same versions with the same checksums, and the same Rust that builds the app for the browser.
    let (by_hand_env, release_env) = (header(BY_HAND, "site"), header(RELEASE, "site"));
    for name in ["TRUNK_VERSION", "TRUNK_SHA256", "WASM_BINDGEN_VERSION", "WASM_BINDGEN_SHA256", "BINARYEN_VERSION", "BINARYEN_SHA256"] {
        assert_eq!(value(&by_hand_env, name), value(&release_env, name), "{name}: move the two workflows together");
    }
    assert_eq!(value(&read(BY_HAND), "RUST_VERSION"), value(&read(RELEASE), "RUST_VERSION"));
    assert!(step(BY_HAND, "site", "Install Rust").contains("--target wasm32-unknown-unknown"));
    // The two folders go to the deploy job as artifacts, as the release workflow's do, hidden files kept.
    for (keep, folder) in [("Keep the pages as an artifact", "site-dist"), ("Keep the web app as an artifact", "app-dist")] {
        let kept = step(BY_HAND, "site", keep);
        assert_eq!(value(&kept, "path"), folder, "{keep}");
        assert_eq!(value(&kept, "include-hidden-files"), "true", "{keep}");
        assert!(kept.contains("uses: actions/upload-artifact@"), "{keep}");
    }
    // It never builds, packages or uploads a download, and does nothing to a release.
    let text = live(BY_HAND).join("\n");
    for banned in
        ["package.sh", "make-deb", "make-appimage", "make-msi", "cargo build", "gh release", "gh api", "contents: write", "packaging/smoke.sh"]
    {
        assert!(!text.contains(banned), "the website's workflow builds no download: `{banned}`");
    }
}

/// The deploy is the release workflow's: the same tool at the same version, installed the same way, the same two projects, and the two secrets read by the two deploy steps alone.
#[test]
fn it_deploys_with_the_pinned_wrangler_to_the_two_projects_and_only_the_two_deploy_steps_read_the_secrets() {
    let install = "Install wrangler, the Cloudflare Pages tool";
    assert_eq!(step(BY_HAND, "deploy", install), step(RELEASE, "deploy", install), "wrangler is installed as the release workflow installs it");
    assert_eq!(
        value(&header(BY_HAND, "deploy"), "WRANGLER_VERSION"),
        value(&header(RELEASE, "deploy"), "WRANGLER_VERSION"),
        "one version of the tool"
    );
    assert_eq!(value(&header(BY_HAND, "deploy"), "WRANGLER_SEND_METRICS"), "false");
    for (name, folder, project) in [
        ("Deploy the pages to Cloudflare Pages", "site-dist", "diacraft-site"),
        ("Deploy the web app to Cloudflare Pages", "app-dist", "diacraft-app"),
    ] {
        let run = value(&step(BY_HAND, "deploy", name), "run");
        assert!(run.starts_with(&format!("wrangler pages deploy {folder} --project-name {project} --branch main ")), "{name}: {run}");
        assert!(run.contains("--commit-hash \"$GITHUB_SHA\"") && run.contains("--commit-dirty=false"), "{name}: {run}");
        let release = value(&step(RELEASE, "deploy", name), "run");
        assert!(
            release.starts_with(&format!("wrangler pages deploy {folder} --project-name {project} --branch main ")),
            "the same project in the release workflow"
        );
    }
    // The order: the folders, the tool, then the two deploys.
    let names: Vec<String> = steps(BY_HAND, "deploy").into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        [
            "Check out the repository, which holds the lock of wrangler",
            "Download the pages the site job made",
            "Download the web app the site job made",
            install,
            "Deploy the pages to Cloudflare Pages",
            "Deploy the web app to Cloudflare Pages",
        ]
    );
    for (get, keep, folder) in [
        ("Download the pages the site job made", "Keep the pages as an artifact", "site-dist"),
        ("Download the web app the site job made", "Keep the web app as an artifact", "app-dist"),
    ] {
        let (got, kept) = (step(BY_HAND, "deploy", get), step(BY_HAND, "site", keep));
        assert_eq!(value(&got, "name"), value(&kept, "name"), "the artifact the deploy takes is the one the build kept");
        assert_eq!(value(&got, "path"), folder);
        assert!(got.contains("uses: actions/download-artifact@"), "{get}");
    }
    // The secrets: each name twice, in the two deploy steps, as `secrets.NAME`, and not in the job's own environment, the install, or any other step or job.
    let live = live(BY_HAND);
    for secret in ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"] {
        assert_eq!(live.iter().filter(|l| l.contains(secret)).count(), 2, "{secret} is read by the two deploy steps and by nothing else");
    }
    assert_eq!(live.iter().filter(|l| l.contains("secrets.")).count(), 4);
    assert!(!header(BY_HAND, "deploy").contains("secrets.") && !job(BY_HAND, "site").contains("secrets."));
    for (name, text) in steps(BY_HAND, "deploy") {
        assert_eq!(text.contains("secrets."), name.starts_with("Deploy the "), "{name}");
    }
    assert!(!live.join("\n").contains("npx") && !live.join("\n").contains("wrangler@"), "the tool comes from the lock, at one version");
}

/// Only GitHub's own actions, each pinned by a full commit, and the same commit as the release workflow pins for the same action; and one concurrency group for the two deploys.
#[test]
fn its_actions_are_githubs_own_pinned_by_commit_and_its_deploy_shares_a_concurrency_group_with_the_release_workflows() {
    let pins = |workflow: &str| -> BTreeMap<String, String> {
        let mut pins = BTreeMap::new();
        for line in live(workflow).iter().filter(|l| l.trim_start().starts_with("uses:") || l.trim_start().starts_with("- uses:")) {
            let spec = line.split("uses:").nth(1).unwrap().trim();
            let (action, rest) = spec.split_once('@').unwrap_or_else(|| panic!("{workflow}: not pinned: {line}"));
            let sha = rest.split_whitespace().next().unwrap();
            assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{workflow}: an action not pinned by a commit: {line}");
            assert!(action.starts_with("actions/"), "{workflow}: an action that is not GitHub's own: {line}");
            pins.insert(action.to_owned(), sha.to_owned());
        }
        pins
    };
    let (by_hand, release) = (pins(BY_HAND), pins(RELEASE));
    assert!(
        by_hand.contains_key("actions/checkout")
            && by_hand.contains_key("actions/upload-artifact")
            && by_hand.contains_key("actions/download-artifact")
    );
    for (action, sha) in &by_hand {
        assert_eq!(release.get(action), Some(sha), "{action}: the two workflows pin one commit");
    }
    // The group is named by both, the run that waits is not cancelled, and no other job of either workflow is in it.
    for workflow in [BY_HAND, RELEASE] {
        let deploy = header(workflow, "deploy");
        assert!(deploy.contains("concurrency:\n      group: deploy-site\n      cancel-in-progress: false"), "{workflow}: {deploy}");
        assert_eq!(live(workflow).iter().filter(|l| l.contains("group: deploy-site")).count(), 1, "{workflow}: one job in the group");
    }
    // The workflow's own group would keep two builds apart for nothing: the build has none.
    assert!(!header(BY_HAND, "site").contains("concurrency"), "only the deploys are serialised");
}

/// A stand-in for `curl` that answers a download from a folder of archives, by the name the address ends with, and writes down each address asked for.
const FAKE_CURL: &str = r#"#!/bin/sh
out=""
url=""
while [ $# -gt 0 ]; do
    case $1 in
    --output) out=$2; shift 2 ;;
    --proto | --retry) shift 2 ;;
    --tlsv1.2 | --fail | --silent | --show-error | --location | --retry-all-errors) shift ;;
    *) url=$1; shift ;;
    esac
done
echo "$url" >>"$ARCHIVES/asked"
cp "$ARCHIVES/$(basename "$url")" "$out"
"#;

/// A stand-in for a tool: a script that says `<what> <version>` for `--version`.
fn tool(says: &str) -> String {
    format!("#!/bin/sh\necho '{says}'\n")
}

fn sha256(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Archives of the three tools as the script unpacks them, made with `tar`, each tool a stand-in that reports a version, and the environment the script reads.
struct Tools {
    dir: TempDir,
    env: BTreeMap<String, String>,
}

impl Tools {
    fn new(trunk: &str, bindgen: &str, wasm_opt: &str) -> Tools {
        let dir = TempDir::new("site-tools");
        let (trunk_v, bindgen_v, binaryen_v) = ("0.22.2", "0.2.130", "version_133");
        let archives = dir.path().join("archives");
        // An archive holds the entries of `top` as the projects' own do: with no `./` before the names.
        let make = |file: &str, top: &str, path: &str, text: String| {
            let stage = dir.path().join("stage").join(file);
            dir.write(&format!("stage/{file}/{path}"), &text);
            std::fs::set_permissions(stage.join(path), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::create_dir_all(&archives).unwrap();
            let status = Command::new("tar").arg("-czf").arg(archives.join(file)).arg("-C").arg(&stage).arg(top).status().unwrap();
            assert!(status.success());
        };
        make("trunk-x86_64-unknown-linux-gnu.tar.gz", "trunk", "trunk", tool(trunk));
        let bindgen_dir = format!("wasm-bindgen-{bindgen_v}-x86_64-unknown-linux-musl");
        make(&format!("{bindgen_dir}.tar.gz"), &bindgen_dir, &format!("{bindgen_dir}/wasm-bindgen"), tool(bindgen));
        let binaryen_dir = format!("binaryen-{binaryen_v}");
        make(&format!("binaryen-{binaryen_v}-x86_64-linux.tar.gz"), &binaryen_dir, &format!("{binaryen_dir}/bin/wasm-opt"), tool(wasm_opt));
        dir.write("bin/curl", FAKE_CURL);
        std::fs::set_permissions(dir.path().join("bin/curl"), std::fs::Permissions::from_mode(0o755)).unwrap();
        dir.write("github_path", "");
        let sum = |file: &str| sha256(&archives.join(file));
        let env: BTreeMap<String, String> = [
            ("TRUNK_VERSION", trunk_v.to_owned()),
            ("TRUNK_SHA256", sum("trunk-x86_64-unknown-linux-gnu.tar.gz")),
            ("WASM_BINDGEN_VERSION", bindgen_v.to_owned()),
            ("WASM_BINDGEN_SHA256", sum(&format!("wasm-bindgen-{bindgen_v}-x86_64-unknown-linux-musl.tar.gz"))),
            ("BINARYEN_VERSION", binaryen_v.to_owned()),
            ("BINARYEN_SHA256", sum(&format!("binaryen-{binaryen_v}-x86_64-linux.tar.gz"))),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        Tools { dir, env }
    }

    fn run(&self, env: &BTreeMap<String, String>) -> std::process::Output {
        let path = format!("{}:/usr/bin:/bin", self.dir.path().join("bin").display());
        let mut command = Command::new("bash");
        command
            .arg(xtask::root().join("packaging/install-site-tools.sh"))
            .env_clear()
            .env("PATH", path)
            .env("ARCHIVES", self.dir.path().join("archives"))
            .env("RUNNER_TEMP", self.dir.path().join("runner"))
            .env("GITHUB_PATH", self.dir.path().join("github_path"));
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }
}

fn say(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn the_tools_script_installs_each_tool_from_its_archive_by_the_checksum_it_was_given_and_puts_them_on_the_path() {
    let tools = Tools::new("trunk 0.22.2", "wasm-bindgen 0.2.130", "wasm-opt version 133 (version_133)");
    let out = tools.run(&tools.env);
    assert!(out.status.success(), "{}", say(&out));
    let bin = tools.dir.path().join("runner/tools/bin");
    for program in ["trunk", "wasm-bindgen", "wasm-opt"] {
        assert!(std::fs::metadata(bin.join(program)).unwrap().permissions().mode() & 0o111 != 0, "{program} is installed and can be run");
    }
    assert_eq!(
        std::fs::read_to_string(tools.dir.path().join("github_path")).unwrap(),
        format!("{}\n", bin.display()),
        "the folder is added to the path of the next steps"
    );
    // Each tool from the address of its release, over HTTPS.
    let asked = std::fs::read_to_string(tools.dir.path().join("archives/asked")).unwrap();
    assert_eq!(
        asked.lines().collect::<Vec<_>>(),
        [
            "https://github.com/trunk-rs/trunk/releases/download/v0.22.2/trunk-x86_64-unknown-linux-gnu.tar.gz",
            "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/0.2.130/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl.tar.gz",
            "https://github.com/WebAssembly/binaryen/releases/download/version_133/binaryen-version_133-x86_64-linux.tar.gz",
        ]
    );
}

#[test]
fn the_tools_script_stops_on_a_wrong_checksum_a_wrong_version_and_a_missing_setting() {
    let tools = Tools::new("trunk 0.22.2", "wasm-bindgen 0.2.130", "wasm-opt version 133 (version_133)");
    // An archive that is not the one the checksum is of: refused before anything is unpacked.
    for name in ["TRUNK_SHA256", "WASM_BINDGEN_SHA256", "BINARYEN_SHA256"] {
        let mut env = tools.env.clone();
        env.insert(name.to_owned(), "0".repeat(64));
        let out = tools.run(&env);
        assert!(!out.status.success(), "{name}: {}", say(&out));
        assert!(say(&out).contains("FAILED") || say(&out).contains("did NOT match"), "{name}: {}", say(&out));
    }
    // A tool that reports another version than the one asked for.
    for (trunk, bindgen, wasm_opt, said) in [
        ("trunk 0.22.1", "wasm-bindgen 0.2.130", "wasm-opt version 133 (version_133)", "trunk reports: trunk 0.22.1"),
        ("trunk 0.22.2", "wasm-bindgen 0.2.129", "wasm-opt version 133 (version_133)", "wasm-bindgen reports: wasm-bindgen 0.2.129"),
        ("trunk 0.22.2", "wasm-bindgen 0.2.130", "wasm-opt version 132 (version_132)", "wasm-opt reports: wasm-opt version 132 (version_132)"),
    ] {
        let tools = Tools::new(trunk, bindgen, wasm_opt);
        let out = tools.run(&tools.env);
        assert!(!out.status.success(), "{said}: {}", say(&out));
        assert!(say(&out).contains(said), "{said}: {}", say(&out));
    }
    // A setting that is missing is an error that names it, and nothing is fetched.
    let fresh = Tools::new("trunk 0.22.2", "wasm-bindgen 0.2.130", "wasm-opt version 133 (version_133)");
    for name in ["TRUNK_VERSION", "WASM_BINDGEN_SHA256", "BINARYEN_VERSION"] {
        let mut env = fresh.env.clone();
        env.remove(name);
        let out = fresh.run(&env);
        assert!(!out.status.success() && say(&out).contains(name), "{name}: {}", say(&out));
    }
    assert!(!fresh.dir.path().join("archives/asked").exists(), "a refused run fetched nothing");
}
