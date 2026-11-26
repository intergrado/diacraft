//! CodeQL's advanced setup (the owner, 2026-11-26): `.github/workflows/codeql.yml` scans the shipped code of the public repository, and
//! `.github/codeql/codeql-config.yml` leaves out the test and fuzz code, which the default setup had flagged (34 alerts, every one dismissed as used
//! in tests). The files are read as text, as the other workflows are: xtask has no YAML reader. Nothing here runs a workflow or reaches GitHub.

use std::collections::BTreeSet;
use std::process::Command;

const WORKFLOW: &str = ".github/workflows/codeql.yml";
const CONFIG: &str = ".github/codeql/codeql-config.yml";
const CI: &str = ".github/workflows/ci.yml";

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The files git tracks, as paths with `/` separators, that are there to be read. Untracked files are left out on purpose: other tests make scratch folders
/// inside the repository for a moment, which `xtask::repo_files` would list and then find gone.
fn tracked_files() -> Vec<String> {
    let out = Command::new("git").current_dir(xtask::root()).args(["ls-files", "-z"]).output().expect("git runs");
    assert!(out.status.success(), "git ls-files: {}", String::from_utf8_lossy(&out.stderr));
    let files: Vec<String> =
        out.stdout.split(|&b| b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8(p.to_vec()).expect("a UTF-8 path")).collect();
    files.into_iter().filter(|f| xtask::root().join(f).is_file()).collect()
}

/// The lines of a file that are not comments or blank, each trimmed on the right.
fn live(rel: &str) -> Vec<String> {
    read(rel).lines().map(str::trim_end).filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty()).map(str::to_owned).collect()
}

/// The lines of the block that starts at the line `header` (at its indent), which ends at the first line that is not indented deeper.
fn block<'a, S: AsRef<str>>(lines: &'a [S], header: &str) -> Vec<&'a str> {
    let at = lines.iter().position(|l| l.as_ref() == header).unwrap_or_else(|| panic!("no `{header}` line"));
    let indent = header.len() - header.trim_start().len();
    lines[at + 1..].iter().map(AsRef::as_ref).take_while(|l| l.len() - l.trim_start().len() > indent).collect()
}

/// The entries of the list at `key:` in the config, unquoted.
fn paths_ignore() -> Vec<String> {
    let lines = live(CONFIG);
    block(&lines, "paths-ignore:")
        .iter()
        .map(|l| l.trim().strip_prefix("- ").unwrap_or_else(|| panic!("not a list entry: {l}")).trim().trim_matches(['\'', '"']).to_owned())
        .collect()
}

// ---- the workflow --------------------------------------------------------------------------------------------------------------------------------

#[test]
fn it_runs_on_pushes_and_pull_requests_to_main_and_every_week_and_never_on_pull_request_target() {
    let lines = live(WORKFLOW);
    let on = block(&lines, "on:");
    let keys: Vec<&str> = on.iter().filter(|l| l.starts_with("  ") && !l.starts_with("   ")).map(|l| l.trim()).collect();
    assert_eq!(keys, ["push:", "pull_request:", "schedule:", "workflow_dispatch:"]);
    for trigger in ["  push:", "  pull_request:"] {
        assert_eq!(block(&lines, trigger), ["    branches: [main]"], "{trigger}");
    }
    // One cron entry: a minute and an hour, no day of the month and no month, and one day of the week.
    let schedule = block(&lines, "  schedule:");
    assert_eq!(schedule.len(), 1, "{schedule:?}");
    let cron = schedule[0].trim().strip_prefix("- cron: ").expect("a cron entry").trim_matches('"');
    let fields: Vec<&str> = cron.split(' ').collect();
    assert_eq!(fields.len(), 5, "{cron}");
    assert!(fields[0].parse::<u8>().is_ok_and(|m| m < 60 && m % 15 != 0), "an odd minute, not the busy ones: {cron}");
    assert!(fields[1].parse::<u8>().is_ok_and(|h| h < 24), "{cron}");
    assert_eq!((fields[2], fields[3]), ("*", "*"), "{cron}");
    assert!(fields[4].parse::<u8>().is_ok_and(|d| d < 7), "one day of the week: {cron}");
    // A run for a fork's pull request must never get the repository's token with write rights over the fork's code.
    assert!(lines.iter().all(|l| !l.contains("pull_request_target")));
}

#[test]
fn it_analyses_rust_the_actions_and_javascript_with_no_build_each_as_a_job_of_its_own() {
    let lines = live(WORKFLOW);
    let include = block(&lines, "        include:");
    let entries: Vec<(&str, &str)> = include
        .chunks(3)
        .map(|c| {
            let language = c[0].trim().strip_prefix("- language: ").expect("an entry starts with its language");
            let mode = c[1].trim().strip_prefix("build-mode: ").expect("then its build mode");
            assert!(c[2].trim().starts_with("timeout: "), "then its time limit: {c:?}");
            (language, mode)
        })
        .collect();
    assert_eq!(entries, [("rust", "none"), ("actions", "none"), ("javascript-typescript", "none")]);
    assert_eq!(include.len(), 9, "three lines for each language");
    // A failed language does not cancel the others, and the languages run as separate jobs (CodeQL's own advice).
    assert!(lines.contains(&"      fail-fast: false".to_owned()));
}

#[test]
fn its_token_may_upload_results_and_read_the_repository_and_nothing_more() {
    let lines = live(WORKFLOW);
    assert_eq!(block(&lines, "permissions:"), ["  contents: read"], "the workflow's own default");
    let job = block(&lines, "  analyze:");
    let permissions = block(&job, "    permissions:").iter().map(|l| l.trim().to_owned()).collect::<BTreeSet<_>>();
    assert_eq!(permissions, ["actions: read", "contents: read", "security-events: write"].map(str::to_owned).into_iter().collect());
}

#[test]
fn its_actions_are_githubs_own_pinned_by_a_full_commit_with_the_version_in_a_comment() {
    let ci_checkout = live(CI).into_iter().find(|l| l.contains("actions/checkout@")).expect("ci.yml checks out");
    let uses: Vec<String> = live(WORKFLOW).into_iter().filter(|l| l.contains("uses:")).collect();
    assert_eq!(uses.len(), 3, "the checkout, init and analyze steps: {uses:?}");
    let mut codeql_commits = BTreeSet::new();
    for line in &uses {
        let spec = line.split("uses:").nth(1).unwrap().trim();
        let (action, rest) = spec.split_once('@').unwrap_or_else(|| panic!("not pinned: {line}"));
        let (sha, comment) = rest.split_once(" # ").unwrap_or_else(|| panic!("no version comment: {line}"));
        assert!(sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')), "not a full commit: {line}");
        let version = comment.strip_prefix('v').unwrap_or_else(|| panic!("the comment is a version, vX.Y.Z: {line}"));
        assert!(version.split('.').count() == 3 && version.split('.').all(|n| n.parse::<u16>().is_ok()), "{line}");
        match action {
            "actions/checkout" => assert!(ci_checkout.contains(&format!("@{sha} # {comment}")), "the commit ci.yml checks out with: {line}"),
            "github/codeql-action/init" | "github/codeql-action/analyze" => {
                // GitHub's own action, at the major version of today (v4), and one commit for both steps.
                assert!(version.starts_with("4."), "{line}");
                codeql_commits.insert((sha.to_owned(), comment.to_owned()));
            }
            other => panic!("an action that is not GitHub's own or not needed: {other}"),
        }
    }
    assert_eq!(codeql_commits.len(), 1, "init and analyze are one release: {codeql_commits:?}");
}

#[test]
fn a_new_push_cancels_a_pull_requests_older_run_only_and_every_job_has_a_time_limit() {
    let lines = live(WORKFLOW);
    let concurrency = block(&lines, "concurrency:");
    assert_eq!(
        concurrency,
        [
            "  group: codeql-${{ github.event_name == 'pull_request' && github.ref || github.run_id }}",
            "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}"
        ]
    );
    let job = block(&lines, "  analyze:");
    assert!(job.contains(&"    timeout-minutes: ${{ matrix.timeout }}"), "{job:?}");
    // The Rust job gets the longest limit, as the gate does (90 minutes), and no job more.
    let timeouts: Vec<u32> = lines.iter().filter_map(|l| l.trim().strip_prefix("timeout: ")).map(|t| t.parse().unwrap()).collect();
    assert_eq!(timeouts.len(), 3);
    assert_eq!(timeouts.iter().max(), Some(&90));
    assert_eq!(timeouts[0], 90, "the first entry is rust");
}

#[test]
fn its_init_step_reads_the_config_and_its_results_have_a_category_for_each_language() {
    let text = live(WORKFLOW).join("\n") + "\n";
    assert!(text.contains("          languages: ${{ matrix.language }}\n          build-mode: ${{ matrix.build-mode }}\n"));
    assert!(text.contains("          config-file: ./.github/codeql/codeql-config.yml\n"));
    assert!(xtask::root().join(CONFIG).is_file());
    // The category the default setup used, so that the results of the two setups are one series.
    assert!(text.contains("          category: \"/language:${{ matrix.language }}\"\n"));
    // The checkout leaves no token in the repository's git configuration, as the other workflows' do.
    assert!(text.contains("        with:\n          persist-credentials: false\n"));
}

// ---- the configuration ---------------------------------------------------------------------------------------------------------------------------

/// Whether `path` matches `pattern` as CodeQL reads its filters: `*` is any run of characters inside one folder or file name, and a `**` segment
/// is any number of folders, at least `min` of them (zero is the reading that matches most, one the reading that matches least).
fn matches(pattern: &str, path: &str, min: usize) -> bool {
    fn segment(pattern: &[u8], name: &[u8]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some((b'*', rest)) => (0..=name.len()).any(|n| segment(rest, &name[n..])),
            Some((c, rest)) => name.split_first().is_some_and(|(d, tail)| c == d && segment(rest, tail)),
        }
    }
    fn go(pattern: &[&str], path: &[&str], min: usize) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            // A `**` that ends a filter stands for what is below a folder: a file is always below it, so at least one name.
            Some((&"**", rest)) => (if rest.is_empty() { min.max(1) } else { min }..=path.len()).any(|n| go(rest, &path[n..], min)),
            Some((p, rest)) => path.split_first().is_some_and(|(name, tail)| segment(p.as_bytes(), name.as_bytes()) && go(rest, tail, min)),
        }
    }
    go(&pattern.split('/').collect::<Vec<_>>(), &path.split('/').collect::<Vec<_>>(), min)
}

#[test]
fn the_matcher_reads_stars_and_double_stars_as_the_filters_of_codeql_do() {
    assert!(matches("**/tests/**", "crates/a/tests/it/x.rs", 0) && matches("**/tests/**", "crates/a/tests/it/x.rs", 1));
    assert!(matches("**/tests/**", "tests/x.rs", 0) && !matches("**/tests/**", "tests/x.rs", 1));
    assert!(!matches("**/tests/**", "crates/a/src/lib.rs", 0));
    assert!(!matches("**/tests/**", "crates/a/tests", 0), "a folder's name alone is not a file under it");
    assert!(matches("crates/*/src/**/tests/**", "crates/a/src/b/tests/c.rs", 1));
    assert!(matches("crates/*/src/**/tests/**", "crates/a/src/tests/c.rs", 0) && !matches("crates/*/src/**/tests/**", "crates/a/src/tests/c.rs", 1));
    assert!(!matches("crates/*/src/**/tests/**", "crates/a/b/src/tests/c.rs", 0), "a star is one folder");
    assert!(matches("**/*_tests.rs", "a/b/batch_tests.rs", 1) && !matches("**/*_tests.rs", "a/b/tests.rs", 0));
    assert!(matches("**/tests.rs", "a/tests.rs", 1) && !matches("**/tests.rs", "a/mytests.rs", 0));
}

#[test]
fn each_filter_follows_the_rules_of_codeql_and_none_is_listed_twice() {
    let filters = paths_ignore();
    assert!(filters.len() >= 8, "{filters:?}");
    assert_eq!(filters.iter().collect::<BTreeSet<_>>().len(), filters.len(), "a filter listed twice: {filters:?}");
    for filter in &filters {
        // GitHub's rules (customizing an advanced setup): `? + [ ] !` are matched literally, so one is a mistake here; `**` stands alone between
        // slashes or at an end, and does not share a name with other characters; a single `*` may.
        assert!(!filter.contains(['?', '+', '[', ']', '!']), "`{filter}`: a character that CodeQL matches literally");
        for (i, part) in filter.split('/').enumerate() {
            assert!(!part.contains("**") || part == "**", "`{filter}`: `**` shares a name with other characters");
            assert!(!part.is_empty(), "`{filter}`: an empty segment (`{i}`)");
        }
        assert!(!filter.starts_with('/') && !filter.ends_with('/'), "`{filter}`");
    }
}

/// Whether a file is test or fuzz code by where it is and what it is called, as the project's conventions have it (AGENTS.md: "Tests may unwrap and
/// panic"; `docs/architecture.md`: the fuzz crates and `testkit`), independently of the filters.
fn is_test_code(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let (name, folders) = parts.split_last().expect("a path has a name");
    folders.iter().any(|f| matches!(*f, "tests" | "fuzz" | "proptest-regressions"))
        || *name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path.starts_with("crates/testkit/")
}

#[test]
fn the_filters_skip_every_test_and_fuzz_source_and_no_shipped_file() {
    let filters = paths_ignore();
    let files = tracked_files();
    let ignored = |file: &str, min: usize| filters.iter().any(|f| matches(f, file, min));
    let (mut tests, mut shipped) = (0, 0);
    for file in &files {
        if is_test_code(file) {
            // The least a reading of `**` ignores must still cover every source of test code.
            if file.ends_with(".rs") {
                tests += 1;
                assert!(ignored(file, 1), "`{file}` is test code and no filter skips it");
            }
        } else {
            shipped += 1;
            // The most a reading ignores must still leave every other file of the repository, the shipped code above all, to be scanned.
            assert!(!ignored(file, 0), "`{file}` is not test code and a filter skips it");
        }
    }
    assert!(tests > 300 && shipped > 1000, "the repository's files were not seen: {tests} tests, {shipped} others");
    // The shipped code the scan is for is there to be scanned.
    for scanned in [
        "crates/engine/src/lib.rs",
        "apps/diacraft/src/main.rs",
        "xtask/src/main.rs",
        "apps/diacraft-web/loader.js",
        "site/download.js",
        ".github/workflows/ci.yml",
    ] {
        assert!(files.iter().any(|f| f == scanned) && !ignored(scanned, 0), "{scanned}");
    }
}

#[test]
fn each_filter_still_names_a_file_of_the_repository() {
    let files = tracked_files();
    for filter in paths_ignore() {
        assert!(files.iter().any(|f| matches(&filter, f, 0)), "the filter `{filter}` names no file of the repository");
    }
}

/// A module file that the filters skip by its name (`tests.rs`, `*_tests.rs`, a folder `tests`) under a crate's `src` is skipped for being test code,
/// so each of them must be one: declared under `#[cfg(test)]` in its parent, never a part of what the program ships. Returns the folder of the module's
/// parent, the module's name and its file's name, for a path of that kind.
fn in_source_test_module(path: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    let src = parts.iter().position(|p| *p == "src")?;
    for (i, part) in parts.iter().enumerate().skip(src + 1) {
        let last = i + 1 == parts.len();
        if (!last && *part == "tests") || (last && (*part == "tests.rs" || part.ends_with("_tests.rs"))) {
            let name = part.strip_suffix(".rs").unwrap_or(part);
            return Some((parts[..i].join("/"), name.to_owned(), (*part).to_owned()));
        }
    }
    None
}

#[test]
fn a_test_module_skipped_by_its_name_is_declared_under_cfg_test() {
    let files = tracked_files();
    let mut seen = BTreeSet::new();
    for file in files.iter().filter(|f| f.ends_with(".rs")) {
        let Some((dir, module, file_name)) = in_source_test_module(file) else { continue };
        if !seen.insert((dir.clone(), module.clone(), file_name.clone())) {
            continue;
        }
        // `mod <name>;` in the parent module's file, and `#[path = "<file>"]` in any file beside it.
        let parents = [format!("{dir}.rs"), format!("{dir}/mod.rs"), format!("{dir}/lib.rs"), format!("{dir}/main.rs")];
        let mut declarations = Vec::new();
        for candidate in files.iter().filter(|f| f.ends_with(".rs")) {
            let beside = candidate.rsplit_once('/').is_some_and(|(d, _)| d == dir);
            let is_parent = parents.contains(candidate);
            if !beside && !is_parent {
                continue;
            }
            let text = read(candidate);
            let lines: Vec<&str> = text.lines().map(str::trim).collect();
            for (at, line) in lines.iter().enumerate() {
                let declares_module = is_parent
                    && (*line == format!("mod {module};") || *line == format!("pub mod {module};") || *line == format!("pub(crate) mod {module};"));
                let declares_path = line.starts_with("#[path = ") && line.contains(&format!("\"{file_name}\""));
                if declares_module || declares_path {
                    declarations.push((candidate.clone(), at, lines[at.saturating_sub(3)..at].contains(&"#[cfg(test)]")));
                }
            }
        }
        assert!(!declarations.is_empty(), "`{file}` is skipped as test code, but no module declares it");
        for (declared_in, at, under_cfg_test) in declarations {
            assert!(
                under_cfg_test,
                "`{file}` is skipped by its name as test code, but `{declared_in}` line {} declares it with no `#[cfg(test)]`",
                at + 1
            );
        }
    }
    assert!(seen.len() >= 30, "the test modules of the sources were not found: {seen:?}");
}

#[test]
fn the_in_source_test_module_reader_names_the_parent_the_module_and_the_file() {
    assert_eq!(in_source_test_module("crates/a/src/b/tests.rs"), Some(("crates/a/src/b".into(), "tests".into(), "tests.rs".into())));
    assert_eq!(
        in_source_test_module("crates/a/src/b/batch_tests.rs"),
        Some(("crates/a/src/b".into(), "batch_tests".into(), "batch_tests.rs".into()))
    );
    assert_eq!(in_source_test_module("apps/a/src/oracle/tests/names.rs"), Some(("apps/a/src/oracle".into(), "tests".into(), "tests".into())));
    assert_eq!(in_source_test_module("crates/a/tests/it/x.rs"), None, "an integration test is no module of the source");
    assert_eq!(in_source_test_module("crates/a/src/lib.rs"), None);
}
