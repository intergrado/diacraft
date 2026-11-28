//! The help page: its layout as written, wrapped and painted, on small pages made for the purpose and on the real one.

use super::*;

fn look(width: Option<usize>, colour: bool) -> Look {
    Look { width, colour }
}

/// The text without its colour codes (`ESC [ ... letter`).
fn strip(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if ('@'..='~').contains(&d) {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}

fn words_of(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

/// The widest line of `text`, in characters.
fn widest(text: &str) -> usize {
    text.lines().map(|line| line.chars().count()).max().unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------------------------------ as written

#[test]
fn a_row_is_padded_to_its_column() {
    let page = [row("t.out", 8, "--out PATH", 24, "where to write")];
    assert_eq!(render(&page, look(None, false)), "        --out PATH      where to write\n");
    // A label that just fits leaves one space; the column is where the words begin, not a number of spaces.
    let page = [row("t.fonts", 8, "--bundled-fonts", 24, "the bundled faces")];
    assert_eq!(render(&page, look(None, false)), "        --bundled-fonts the bundled faces\n");
    // One row of the page has always been a space short of its column (`--via diacraft`, whose words begin at 23).
    let page = [row("t.via", 8, "--via diacraft", 23, "the trip")];
    assert_eq!(render(&page, look(None, false)), "        --via diacraft the trip\n");
}

#[test]
fn a_label_too_long_for_its_column_leaves_the_words_to_the_next_line() {
    let page = [row("t.root", 8, "--automation-read-root DIR", 24, "keep the files\nto this folder")];
    assert_eq!(
        render(&page, look(None, false)),
        "        --automation-read-root DIR\n                        keep the files\n                        to this folder\n"
    );
}

#[test]
fn the_lines_of_a_description_hang_at_the_column() {
    let page = [row("t.cmds", 8, "--commands SRC", 24, "a file, or - for standard input,\nholding steps")];
    assert_eq!(render(&page, look(None, false)), "        --commands SRC  a file, or - for standard input,\n                        holding steps\n");
}

#[test]
fn a_paragraph_keeps_the_line_breaks_its_text_has_at_its_indent() {
    let page = [para("t.p", 6, "first line\nsecond line")];
    assert_eq!(render(&page, look(None, false)), "      first line\n      second line\n");
    let page = [blank(), heading("t.h", "Commands:"), synopsis("diacraft-cli mcp [--headless]")];
    assert_eq!(render(&page, look(None, false)), "\nCommands:\n  diacraft-cli mcp [--headless]\n");
}

#[test]
fn a_lead_puts_its_heading_before_its_words_and_hangs_the_rest_at_no_indent() {
    let page = [lead("t.fonts", "Fonts:", "`run` and `export` read\nthe fonts once.")];
    assert_eq!(render(&page, look(None, false)), "Fonts: `run` and `export` read\nthe fonts once.\n");
    let page = [usage("t.usage", "Usage:", "diacraft-cli <command> [options]")];
    assert_eq!(render(&page, look(None, false)), "Usage: diacraft-cli <command> [options]\n");
}

#[test]
fn an_environment_variable_is_a_row_like_an_option() {
    let page = [variable("t.env", 2, "RUST_LOG", 17, "what is logged")];
    assert_eq!(render(&page, look(None, false)), "  RUST_LOG       what is logged\n");
}

/// The page as it is written is the entries' own text, line by line, which is what a pipe has always been given.
#[test]
fn the_real_page_as_written_has_the_text_of_every_entry_in_order() {
    let mut expected = String::new();
    for entry in PAGE {
        match *entry {
            Entry::Blank => expected.push('\n'),
            Entry::Heading { text, .. } => expected.push_str(&format!("{text}\n")),
            Entry::Lead { label, text, .. } => expected.push_str(&format!("{label} {text}\n")),
            Entry::Synopsis { text } => expected.push_str(&format!("  {text}\n")),
            Entry::Para { indent, text, .. } => {
                for line in text.split('\n') {
                    expected.push_str(&format!("{}{line}\n", " ".repeat(usize::from(indent))));
                }
            }
            Entry::Row { indent, label, col, text, .. } => {
                let first = indent as usize + label.chars().count();
                let mut lines = text.split('\n');
                let head = if first < usize::from(col) {
                    format!("{}{label}{}{}", " ".repeat(indent as usize), " ".repeat(usize::from(col) - first), lines.next().unwrap_or(""))
                } else {
                    format!("{}{label}\n{}{}", " ".repeat(indent as usize), " ".repeat(usize::from(col)), lines.next().unwrap_or(""))
                };
                expected.push_str(&format!("{head}\n"));
                for line in lines {
                    expected.push_str(&format!("{}{line}\n", " ".repeat(usize::from(col))));
                }
            }
        }
    }
    assert_eq!(plain(), expected);
    assert!(plain().starts_with("Usage: diacraft-cli <command> [options]\n\nCommands:\n  diacraft-cli mcp "), "{}", plain());
    assert!(!plain().contains('\u{1b}') && plain().ends_with("\n") && !plain().ends_with("\n\n"));
}

// ------------------------------------------------------------------------------------------------------------------------ wrapped

#[test]
fn a_paragraph_is_wrapped_at_the_width_and_keeps_its_indent() {
    let page = [para("t.p", 6, "one two three four five six seven eight nine ten\neleven twelve")];
    let wrapped = render(&page, look(Some(24), false));
    for line in wrapped.lines() {
        assert!(line.starts_with("      ") && !line.starts_with("       "), "{line:?}");
        assert!(line.chars().count() <= 24, "{line:?}");
    }
    assert!(wrapped.lines().count() > 2, "{wrapped}");
    assert_eq!(words_of(&wrapped), words_of("one two three four five six seven eight nine ten eleven twelve"));
    // The line break of the text is no break of the wrapped page: the words flow.
    assert_eq!(render(&page, look(Some(200), false)), "      one two three four five six seven eight nine ten eleven twelve\n");
}

#[test]
fn a_word_wider_than_the_room_is_left_to_overflow_on_a_line_of_its_own() {
    let page = [para("t.p", 2, "see /a/very/long/path/that/does/not/fit/anywhere now")];
    let wrapped = render(&page, look(Some(20), false));
    assert_eq!(wrapped, "  see\n  /a/very/long/path/that/does/not/fit/anywhere\n  now\n");
}

#[test]
fn a_wrapped_row_keeps_its_label_and_hangs_the_words_at_the_column() {
    let page = [row("t.r", 8, "--out PATH", 24, "where to write (replaced if it is there; never the drawing itself)")];
    let wrapped = render(&page, look(Some(50), false));
    let lines: Vec<&str> = wrapped.lines().collect();
    assert!(lines[0].starts_with("        --out PATH      where to write"), "{wrapped}");
    for line in &lines[1..] {
        assert!(line.starts_with(&" ".repeat(24)) && !line.starts_with(&" ".repeat(25)), "{line:?}");
    }
    assert!(widest(&wrapped) <= 50, "{wrapped}");
}

#[test]
fn a_wrapped_label_that_is_too_long_still_leaves_its_words_to_the_next_line() {
    let page = [row("t.r", 8, "--automation-read-root DIR", 24, "keep the files to this folder and the folders in it")];
    let wrapped = render(&page, look(Some(50), false));
    assert!(wrapped.starts_with("        --automation-read-root DIR\n                        keep the files"), "{wrapped}");
}

#[test]
fn a_bracket_group_of_a_usage_line_is_not_broken_and_the_rest_hangs_at_6() {
    let page = [synopsis("diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move | --move DX,DY] [--bundled-fonts]")];
    let wrapped = render(&page, look(Some(50), false));
    assert!(wrapped.contains("[--edit cell|move | --move DX,DY]"), "a group is one word: {wrapped}");
    let lines: Vec<&str> = wrapped.lines().collect();
    assert!(lines[0].starts_with("  diacraft-cli roundtrip"), "{wrapped}");
    for line in &lines[1..] {
        assert!(line.starts_with("      ") && !line.starts_with("       "), "{line:?}");
    }
    assert!(lines.len() > 1 && widest(&wrapped) <= 50, "{wrapped}");
}

#[test]
fn a_lead_is_wrapped_after_its_heading_with_the_rest_at_no_indent() {
    let page = [lead("t.f", "Fonts:", "`run` and `export` and `convert` read the machine's fonts once, at the start")];
    let wrapped = render(&page, look(Some(30), false));
    assert!(wrapped.starts_with("Fonts: `run` and `export` and\n"), "{wrapped}");
    assert!(wrapped.lines().skip(1).all(|line| !line.starts_with(' ')), "{wrapped}");
}

#[test]
fn the_real_page_wrapped_has_the_words_of_the_page_and_fits_the_width() {
    let plain = plain();
    // Down to 40 columns the words are the page's; from 60 no line is wider than the terminal (the longest word of the page, a group of a usage line, is 51 columns and its indent 6).
    for width in [40, 60, 80, 100, 120] {
        let wrapped = render(PAGE, look(Some(width), false));
        assert_eq!(words_of(&wrapped), words_of(&plain), "width {width}");
        if width >= 60 {
            assert!(widest(&wrapped) <= width, "width {width}: a line of {}", widest(&wrapped));
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------ painted

fn roles(spans: &[(Role, &str)]) -> Vec<(Role, String)> {
    spans.iter().map(|(role, text)| (*role, (*text).to_owned())).collect()
}

fn pair(role: Role, text: &str) -> (Role, String) {
    (role, text.to_owned())
}

#[test]
fn prose_marks_the_options_and_the_names_in_backticks_and_nothing_else() {
    assert_eq!(
        roles(&prose("use --out PATH and `diacraft-cli run`, not -x1, a-b, non-empty, JSON-RPC, -, -- or --9")),
        [
            pair(Role::Plain, "use "),
            pair(Role::Literal, "--out"),
            pair(Role::Plain, " PATH and "),
            pair(Role::Literal, "`diacraft-cli run`"),
            pair(Role::Plain, ", not -x1, a-b, non-empty, JSON-RPC, -, -- or --9"),
        ]
    );
    assert_eq!(
        roles(&prose("(--bundled-fonts) and -h, -V.")),
        [
            pair(Role::Plain, "("),
            pair(Role::Literal, "--bundled-fonts"),
            pair(Role::Plain, ") and "),
            pair(Role::Literal, "-h"),
            pair(Role::Plain, ", "),
            pair(Role::Literal, "-V"),
            pair(Role::Plain, "."),
        ]
    );
    // `--out=PATH` is the option and then its value; a path with a dash in it is no option; a backtick with no mate marks nothing.
    assert_eq!(roles(&prose("--out=a-b ./a/--b it's `loose")), [pair(Role::Literal, "--out"), pair(Role::Plain, "=a-b ./a/--b it's `loose")]);
    assert_eq!(roles(&prose("")), []);
    assert_eq!(roles(&prose("plain words")), [pair(Role::Plain, "plain words")]);
}

#[test]
fn a_usage_line_marks_names_and_the_places_a_person_fills_in() {
    let spans = usage_spans("diacraft-cli export FILE --out PATH [--text text|outlines] DX,DY <command> [options] FILE|- REPORTS...", 2);
    let names: Vec<(Role, &str)> = spans.iter().copied().filter(|(role, _)| *role != Role::Plain).collect();
    assert_eq!(
        names,
        [
            (Role::Literal, "diacraft-cli"),
            (Role::Literal, "export"),
            (Role::Placeholder, "FILE"),
            (Role::Literal, "--out"),
            (Role::Placeholder, "PATH"),
            (Role::Literal, "--text"),
            (Role::Literal, "text"),
            (Role::Literal, "outlines"),
            (Role::Placeholder, "DX,DY"),
            (Role::Placeholder, "<command>"),
            (Role::Placeholder, "options"),
            (Role::Placeholder, "FILE"),
            (Role::Placeholder, "REPORTS"),
        ]
    );
    // The brackets, bars, dashes and dots between are plain, and the text is all there, in order.
    assert_eq!(
        spans.iter().map(|(_, text)| *text).collect::<String>(),
        "diacraft-cli export FILE --out PATH [--text text|outlines] DX,DY <command> [options] FILE|- REPORTS..."
    );
}

#[test]
fn an_option_label_marks_its_option_and_its_place_and_a_comma_is_plain() {
    let spans = usage_spans("-V, --version", 0);
    assert_eq!(roles(&spans), [pair(Role::Literal, "-V"), pair(Role::Plain, ","), pair(Role::Plain, " "), pair(Role::Literal, "--version")]);
    // A variable's name is a name, wholly, and so are two of them.
    let spans = usage_spans("A_ROOT, B_ROOT", usize::MAX);
    assert_eq!(roles(&spans), [pair(Role::Literal, "A_ROOT"), pair(Role::Plain, ","), pair(Role::Plain, " "), pair(Role::Literal, "B_ROOT")]);
}

#[test]
fn painting_adds_codes_and_only_codes() {
    let plain = plain();
    let painted = render(PAGE, look(None, true));
    assert!(painted.contains('\u{1b}'), "no colour");
    assert_eq!(strip(&painted), plain);
    for width in [40, 80, 120] {
        assert_eq!(strip(&render(PAGE, look(Some(width), true))), render(PAGE, look(Some(width), false)), "width {width}");
    }
    // Headings, names and places are each painted somewhere on the page.
    for style in [Palette::HEADING, Palette::LITERAL, Palette::PLACEHOLDER] {
        assert!(painted.contains(&format!("{style}")), "{style:?}");
    }
}

#[test]
fn a_heading_is_painted_whole_and_a_backtick_name_with_its_backticks() {
    let page = [heading("t.h", "Commands:"), para("t.p", 0, "run `diacraft-cli run` or --cmd now")];
    let painted = render(&page, look(None, true));
    let heading_style = Palette::HEADING;
    let literal = Palette::LITERAL;
    assert!(painted.starts_with(&format!("{heading_style}Commands:{heading_style:#}\n")), "{painted:?}");
    assert!(painted.contains(&format!("{literal}`diacraft-cli run`{literal:#}")), "{painted:?}");
    assert!(painted.contains(&format!("{literal}--cmd{literal:#}")), "{painted:?}");
}

// ------------------------------------------------------------------------------------------------------------------------ the table

#[test]
fn every_keyed_piece_has_its_own_key() {
    let mut seen = std::collections::HashSet::new();
    for entry in PAGE {
        let key = match *entry {
            Entry::Heading { key, .. } | Entry::Lead { key, .. } | Entry::Para { key, .. } | Entry::Row { key, .. } => key,
            Entry::Blank | Entry::Synopsis { .. } => continue,
        };
        assert!(!key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')), "{key:?}");
        assert!(seen.insert(key), "the key {key} is used twice");
    }
    assert!(seen.len() > 60, "{} keyed pieces", seen.len());
}

#[test]
fn a_row_begins_its_words_after_its_label_and_a_text_has_no_stray_spaces() {
    for entry in PAGE {
        let (text, label, indent, col) = match *entry {
            Entry::Row { text, label, indent, col, .. } => (text, Some(label), usize::from(indent), usize::from(col)),
            Entry::Para { text, .. } | Entry::Lead { text, .. } | Entry::Heading { text, .. } => (text, None, 0, 0),
            Entry::Synopsis { text } => (text, None, 0, 0),
            Entry::Blank => continue,
        };
        if let Some(label) = label {
            assert!(col > indent && col - indent <= 24, "{label}: column {col}");
            assert!(!label.contains("  ") && !label.ends_with(' '), "{label:?}");
        }
        assert!(!text.is_empty() && !text.starts_with(' ') && !text.ends_with(' ') && !text.ends_with('\n'), "{text:?}");
        for line in text.split('\n') {
            assert!(!line.is_empty() && !line.starts_with(' ') && !line.ends_with(' ') && !line.contains("  "), "a line of {text:?}: {line:?}");
        }
    }
}

#[test]
fn describe_gives_the_sentence_of_a_key_on_one_line() {
    assert_eq!(describe("export.out"), "where to write (replaced if it is there; never the drawing itself). Written whole or not at all.");
    assert!(describe("run.commands").starts_with("a file, or - for standard input, holding steps in one of two forms. A JSON array of step objects"));
    assert!(!describe("run.commands").contains('\n'));
    assert_eq!(describe("no.such.key"), "");
    assert_eq!(describe(""), "");
}

#[test]
fn the_paragraphs_of_a_command_are_its_own_and_in_order() {
    let run = paragraphs("run");
    assert_eq!(run.len(), 2, "{run:?}");
    assert!(run[0].starts_with("Runs commands one after another"));
    assert!(run[1].starts_with("A run holds at most 100000 steps"));
    assert!(paragraphs("oracle").iter().all(|p| !p.starts_with("Runs commands")));
    assert_eq!(paragraphs("nonesuch"), Vec::<String>::new());
    // A command whose name begins another's does not take its paragraphs.
    assert!(paragraphs("man").iter().all(|p| !p.contains("completion script")));
}

#[test]
fn a_message_is_its_english_until_a_catalogue_stands_in() {
    assert_eq!(message("t.k", "English words"), "English words");
}
