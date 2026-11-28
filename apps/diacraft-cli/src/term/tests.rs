//! The rules of colour and the paint.

use anstyle::Style;

use super::{Palette, When, clamp_width, colours, message, paint};

/// The environment of a test: `pairs` are set, anything else is not.
fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| pairs.iter().find(|pair| pair.0 == name).map(|pair| pair.1.to_owned())
}

#[test]
fn the_flag_decides_before_the_environment_does() {
    let hostile = env(&[("NO_COLOR", "1"), ("CLICOLOR", "0"), ("TERM", "dumb")]);
    assert!(colours(When::Always, &hostile, false), "--color always colours a pipe, whatever the variables say");
    let forcing = env(&[("CLICOLOR_FORCE", "1"), ("TERM", "xterm")]);
    assert!(!colours(When::Never, &forcing, true), "--color never does not colour a terminal, whatever the variables say");
}

#[test]
fn a_pipe_is_plain_and_a_terminal_is_coloured_when_nothing_says_otherwise() {
    assert!(!colours(When::Auto, &env(&[("TERM", "xterm-256color")]), false));
    assert!(colours(When::Auto, &env(&[("TERM", "xterm-256color")]), true));
    assert!(!colours(When::Auto, &env(&[]), false));
}

#[test]
fn no_color_wins_over_everything_but_the_flag_and_an_empty_one_is_not_set() {
    for terminal in [false, true] {
        assert!(!colours(When::Auto, &env(&[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1"), ("TERM", "xterm")]), terminal), "terminal {terminal}");
    }
    assert!(!colours(When::Auto, &env(&[("NO_COLOR", "anything"), ("TERM", "xterm")]), true));
    assert!(colours(When::Auto, &env(&[("NO_COLOR", ""), ("TERM", "xterm")]), true), "NO_COLOR set to nothing is not set");
    assert!(colours(When::Auto, &env(&[("NO_COLOR", ""), ("CLICOLOR_FORCE", "1")]), false));
}

#[test]
fn clicolor_force_colours_a_pipe_unless_it_is_nothing_or_0() {
    assert!(colours(When::Auto, &env(&[("CLICOLOR_FORCE", "1")]), false));
    assert!(colours(When::Auto, &env(&[("CLICOLOR_FORCE", "yes"), ("TERM", "dumb")]), false), "it forces past a dumb terminal too");
    assert!(!colours(When::Auto, &env(&[("CLICOLOR_FORCE", "0")]), false));
    assert!(!colours(When::Auto, &env(&[("CLICOLOR_FORCE", "")]), false));
    // CLICOLOR_FORCE=0 says nothing: the terminal's own rules go on.
    assert!(colours(When::Auto, &env(&[("CLICOLOR_FORCE", "0"), ("TERM", "xterm")]), true));
}

#[test]
fn clicolor_0_and_a_dumb_terminal_are_plain() {
    assert!(!colours(When::Auto, &env(&[("CLICOLOR", "0"), ("TERM", "xterm")]), true));
    assert!(colours(When::Auto, &env(&[("CLICOLOR", "1"), ("TERM", "xterm")]), true), "any CLICOLOR but 0 is no objection");
    assert!(!colours(When::Auto, &env(&[("TERM", "dumb")]), true));
    // CLICOLOR_FORCE comes before CLICOLOR: the force wins.
    assert!(colours(When::Auto, &env(&[("CLICOLOR", "0"), ("CLICOLOR_FORCE", "1")]), false));
}

#[test]
fn a_terminal_that_names_no_term_is_plain_except_on_windows() {
    assert_eq!(colours(When::Auto, &env(&[]), true), cfg!(windows));
    assert_eq!(colours(When::Auto, &env(&[("TERM", "")]), true), cfg!(windows), "an empty TERM names none");
}

#[test]
fn the_choices_are_the_three_words_in_lower_case() {
    assert_eq!(When::WORDS, ["auto", "always", "never"]);
    for (word, when) in [("auto", When::Auto), ("always", When::Always), ("never", When::Never)] {
        assert_eq!(When::of(word), Some(when));
    }
    for word in ["", "Always", "ALWAYS", "yes", "no", "true", "1", " auto", "auto "] {
        assert_eq!(When::of(word), None, "{word:?}");
    }
    assert_eq!(When::default(), When::Auto);
}

#[test]
fn paint_wraps_the_text_in_its_style_only_when_asked() {
    assert_eq!(paint(false, Palette::ERROR, "failed"), "failed");
    let painted = paint(true, Palette::ERROR, "failed");
    assert!(painted.starts_with('\u{1b}') && painted.contains("failed") && painted.ends_with("\u{1b}[0m"), "{painted:?}");
    assert_eq!(paint(true, Style::new(), "plain"), "plain", "no style, no codes");
}

#[test]
fn a_message_is_the_program_name_a_space_and_the_words() {
    assert_eq!(message(false, "step 0 failed"), "diacraft-cli: step 0 failed\n");
    let coloured = message(true, "step 0 failed");
    assert!(coloured.ends_with(" step 0 failed\n") && coloured.contains("diacraft-cli:") && coloured.starts_with('\u{1b}'), "{coloured:?}");
}

#[test]
fn the_page_is_wrapped_to_a_width_between_40_and_120() {
    assert_eq!(clamp_width(None), None, "what is not a terminal is not wrapped");
    assert_eq!(clamp_width(Some(10)), Some(40));
    assert_eq!(clamp_width(Some(40)), Some(40));
    assert_eq!(clamp_width(Some(80)), Some(80));
    assert_eq!(clamp_width(Some(120)), Some(120));
    assert_eq!(clamp_width(Some(400)), Some(120));
}
