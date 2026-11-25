//! The CodeRabbit configuration (`.coderabbit.yaml` at the root, which the export publishes): its review is advice and never blocks a
//! merge, its path filters only take files out of a review and each still names something in the repository, it reads the project's
//! rules, and the rules say how its findings are handled. The file is read as text, as the workflows are: xtask has no YAML reader.

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The lines of the file with the comments left out, each with its text trimmed on the right.
fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim_end).filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty()).collect()
}

/// The entries (`- "text"`, unquoted) of the list that starts at the line `key:`, which ends at the first line that is not an entry.
fn list(text: &str, key: &str) -> Vec<String> {
    let all = lines(text);
    let at = all.iter().position(|l| l.trim() == format!("{key}:")).unwrap_or_else(|| panic!("the file has no `{key}:` list"));
    all[at + 1..].iter().map_while(|l| l.trim_start().strip_prefix("- ")).map(|entry| entry.trim().trim_matches('"').to_owned()).collect()
}

/// Whether a path filter (without its `!`) still names a file: a folder of that name for `**/<folder>/**`, a file that starts with the
/// text before its first `*` for any other.
fn names_a_file(pattern: &str, files: &[String]) -> bool {
    if let Some(rest) = pattern.strip_prefix("**/") {
        let folder = rest.trim_end_matches("/**");
        return files.iter().any(|f| f.rsplit_once('/').is_some_and(|(dir, _)| dir.split('/').any(|part| part == folder)));
    }
    let prefix = pattern.split('*').next().unwrap_or_default();
    files.iter().any(|f| f.starts_with(prefix))
}

#[test]
fn a_path_filter_that_names_nothing_is_found() {
    let files = ["a/b/c.txt".to_owned(), "x.txt".to_owned()];
    assert!(names_a_file("a/**", &files));
    assert!(names_a_file("x*.txt", &files));
    assert!(names_a_file("**/b/**", &files));
    assert!(!names_a_file("gone/**", &files));
    assert!(!names_a_file("**/c.txt/**", &files), "a file is not a folder");
    assert!(!names_a_file("**/a/**", &["x/y.txt".to_owned()]));
}

#[test]
fn the_path_filters_only_remove_files_and_each_names_something_in_the_repository() {
    let config = read(".coderabbit.yaml");
    let filters = list(&config, "path_filters");
    assert!(filters.len() >= 5, "{filters:?}");
    let files = xtask::repo_files(&xtask::root()).unwrap();
    for filter in &filters {
        // One pattern without `!` turns the list into an allowlist, and everything else goes unreviewed.
        let pattern = filter.strip_prefix('!').unwrap_or_else(|| panic!("the path filter `{filter}` does not start with `!`"));
        assert!(names_a_file(pattern, &files), "the path filter `{filter}` names no file of the repository");
    }
    // The rules the reviewer is pointed at must stay reviewable.
    for filter in &filters {
        assert!(
            filter != "!docs/**" && !filter.contains("AGENTS.md") && !filter.contains("development.md"),
            "the path filter `{filter}` takes out a rules file"
        );
    }
}

#[test]
fn the_review_is_advice_and_never_blocks_a_merge() {
    let config = read(".coderabbit.yaml");
    let all = lines(&config);
    let has = |line: &str| all.iter().any(|l| l.trim() == line);
    // No request for changes and no automatic approval, which is the one place a finding can hold a merge back.
    let at = all.iter().position(|l| l.trim() == "auto_approvals:").expect("auto_approvals is set");
    assert_eq!(all[at + 1].trim(), "enabled: false");
    assert!(!has("request_changes_workflow: true"));
    // A review that fails ends as neutral, and no check of the pre-merge kind is an error.
    assert!(has("fail_commit_status: false"));
    assert!(all.iter().all(|l| !l.contains("mode: error")), "a pre-merge check is set to error");
    // Calm, as the owner asked.
    assert!(has("poem: false"));
    assert!(has("profile: chill") || has("profile: quiet"));
    // Non-draft pull requests are reviewed on their own (the default branch is always one of them).
    let at = all.iter().position(|l| l.trim() == "auto_review:").expect("auto_review is set");
    assert_eq!((all[at + 1].trim(), all[at + 2].trim()), ("enabled: true", "drafts: false"));
    assert!(has("language: en-US"));
}

#[test]
fn the_reviewer_is_pointed_at_the_rules_and_the_development_notes() {
    let config = read(".coderabbit.yaml");
    let patterns = list(&config, "filePatterns");
    assert_eq!(patterns, ["AGENTS.md", "docs/development.md"]);
    for file in &patterns {
        assert!(xtask::root().join(file).is_file(), "{file} is not in the repository");
    }
}

/// The owner's ruling (2026-11-25): CodeRabbit reviews the public repository's pull requests, for advice only, and the rules say so in the
/// private file and in the public one (the published tree has the public file as `AGENTS.md`, and only that one).
#[test]
fn the_rules_say_that_the_review_is_advice_and_that_a_real_finding_is_fixed_in_the_source() {
    let mut read_any = false;
    for rel in ["AGENTS.md", "public/AGENTS.md"] {
        if !xtask::root().join(rel).exists() {
            continue;
        }
        read_any = true;
        let rules = read(rel);
        // The paragraph is one run of text, whatever the line breaks of the file.
        let words = rules.split_whitespace().collect::<Vec<_>>().join(" ");
        for needed in ["CodeRabbit", "`.coderabbit.yaml`", "never blocks a merge", "not on the public branch"] {
            assert!(words.contains(needed), "{rel} does not say `{needed}`");
        }
    }
    assert!(read_any, "the rules file is there");
}
