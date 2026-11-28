//! The manual pages.

use super::*;

fn page_of(name: &str) -> String {
    output(&ManArgs::Page(name.to_owned())).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn the_pages_are_the_program_the_app_and_one_for_each_command() {
    let names = pages();
    assert_eq!(names.len(), 2 + cli::COMMANDS.len());
    assert_eq!(&names[..2], ["diacraft-cli", "diacraft"]);
    for command in cli::COMMANDS {
        assert!(names.contains(&format!("diacraft-cli-{command}")), "{command}");
    }
    let unique: std::collections::BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
    assert_eq!(output(&ManArgs::List).unwrap(), names.iter().map(|n| format!("{n}\n")).collect::<String>());
}

#[test]
fn every_page_is_a_manual_page_that_is_titled_by_its_name() {
    for name in pages() {
        let page = page_of(&name);
        let title = name.replace('-', "\\-");
        assert!(page.contains(&format!(".TH {name} 1")), "{name}: no title in {}", page.lines().take(6).collect::<Vec<_>>().join(" / "));
        assert!(page.contains(".SH NAME") && page.contains(".SH SYNOPSIS") && page.contains(&title), "{name}");
        assert!(!page.contains('\u{1b}'), "{name}: a terminal escape");
        assert!(page.ends_with('\n'), "{name}");
    }
}

#[test]
fn a_commands_page_carries_the_words_of_the_help_page() {
    let run = page_of("diacraft-cli-run");
    assert!(run.contains("Runs commands one after another"), "{run}");
    assert!(run.contains("A run holds at most 100000 steps"), "{run}");
    assert!(run.contains("\\-\\-cmd"), "the options are listed: {run}");
    assert!(run.contains("a command id, such as file.new or page.add"), "an option has its sentence of the table: {run}");
    let export = page_of("diacraft-cli-export");
    assert!(export.contains("the pixels to the inch of a PNG"), "{export}");
}

#[test]
fn the_program_page_lists_the_commands_and_the_app_page_the_app_s_options() {
    let program = page_of("diacraft-cli");
    for command in cli::COMMANDS {
        assert!(program.contains(command), "{command}");
    }
    let app = page_of("diacraft");
    assert!(app.contains("\\-\\-control") && !app.contains("roundtrip"), "{app}");
}

#[test]
fn a_page_that_is_not_one_is_refused_in_words() {
    for name in ["diacraft-cli-nonesuch", "nonesuch", "", "diacraft-cli-", "Diacraft-cli", "diacraft-cli-run "] {
        let error = output(&ManArgs::Page(name.to_owned())).expect_err(name);
        assert!(error.contains("is not a manual page"), "{name:?}: {error}");
    }
}
