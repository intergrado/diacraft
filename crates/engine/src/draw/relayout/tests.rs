//! Unit tests of the layout settings: the codes of `PlaceStyle`, the choice of a style by name.

use diacraft_layout::{Align, Direction, Side, Style};

use super::{alignment_name, choose, direction_name, kind_name, place_code, style_of_code};

#[test]
fn every_place_style_code_is_the_style_it_stands_for_and_back() {
    // The table: 1 top to bottom, 2 left to right, 3 radial, 4 bottom to top, 5 right to left, 6 circular, 7 to 14 compact trees, 16 to 27 hierarchies. Catches a table that
    // is off by one, and two styles with one code.
    let mut seen = std::collections::HashSet::new();
    for code in (1..=14).chain(16..=27) {
        let style = style_of_code(code);
        assert_eq!(place_code(style), code, "{code}: {style:?}");
        assert!(seen.insert(code));
    }
    assert_eq!(style_of_code(1), Style::Flowchart(Direction::TopToBottom));
    assert_eq!(style_of_code(2), Style::Flowchart(Direction::LeftToRight));
    assert_eq!(style_of_code(4), Style::Flowchart(Direction::BottomToTop));
    assert_eq!(style_of_code(5), Style::Flowchart(Direction::RightToLeft));
    assert_eq!(style_of_code(3), Style::Radial);
    assert_eq!(style_of_code(6), Style::Circular);
    assert_eq!(style_of_code(7), Style::CompactTree { run: Side::Down, level: Side::Right });
    assert_eq!(style_of_code(14), Style::CompactTree { run: Side::Down, level: Side::Left });
    assert_eq!(style_of_code(16), Style::Hierarchy(Direction::TopToBottom, Align::Start));
    assert_eq!(style_of_code(18), Style::Hierarchy(Direction::TopToBottom, Align::End));
    assert_eq!(style_of_code(19), Style::Hierarchy(Direction::BottomToTop, Align::Start));
    assert_eq!(style_of_code(24), Style::Hierarchy(Direction::LeftToRight, Align::End));
    assert_eq!(style_of_code(27), Style::Hierarchy(Direction::RightToLeft, Align::End));
}

#[test]
fn a_code_that_is_no_style_is_a_flowchart_down_the_page() {
    // 0 is the page's default, 15 is the parent's (a shape's cell), and nothing is past 27.
    for code in [0, 15, 28, 100, 255] {
        assert_eq!(style_of_code(code), Style::Flowchart(Direction::TopToBottom), "{code}");
    }
}

#[test]
fn a_style_is_chosen_by_name_and_the_page_fills_in_what_the_call_leaves_out() {
    let page = Style::Hierarchy(Direction::LeftToRight, Align::End);
    // Nothing given: the page's own. Only a direction: the page's kind and alignment. Another kind: that kind's first choices, not the page's direction.
    assert_eq!(choose(None, None, None, page), Ok(page));
    assert_eq!(choose(None, Some("top_to_bottom"), None, page), Ok(Style::Hierarchy(Direction::TopToBottom, Align::End)));
    assert_eq!(choose(Some("flowchart"), None, None, page), Ok(Style::Flowchart(Direction::TopToBottom)));
    assert_eq!(choose(Some("hierarchy"), None, Some("middle"), page), Ok(Style::Hierarchy(Direction::LeftToRight, Align::Center)));
    assert_eq!(choose(Some("compact_tree"), None, None, page), Ok(Style::CompactTree { run: Side::Down, level: Side::Right }));
    assert_eq!(choose(Some("compact_tree"), Some("up_left"), None, page), Ok(Style::CompactTree { run: Side::Up, level: Side::Left }));
    assert_eq!(choose(Some("radial"), None, None, page), Ok(Style::Radial));
    assert_eq!(choose(Some("circular"), None, None, page), Ok(Style::Circular));
}

#[test]
fn a_name_that_is_not_a_choice_and_a_choice_a_kind_has_none_of_are_refused_in_words() {
    let page = Style::Radial;
    for (kind, direction, alignment, words) in [
        (Some("tree"), None, None, "`style` \"tree\" is not one of"),
        (Some("flowchart"), Some("sideways"), None, "`direction` \"sideways\""),
        (Some("compact_tree"), Some("top_to_bottom"), None, "`direction` of a compact tree"),
        (Some("radial"), Some("top_to_bottom"), None, "a radial layout has none"),
        (Some("flowchart"), None, Some("start"), "`alignment` is for a hierarchy"),
        (Some("hierarchy"), None, Some("sideways"), "`alignment` \"sideways\""),
    ] {
        let err = choose(kind, direction, alignment, page).unwrap_err();
        assert!(err.contains(words), "{kind:?} {direction:?} {alignment:?}: {err}");
    }
}

#[test]
fn the_names_a_command_answers_with_are_the_names_it_takes() {
    for code in (1..=14).chain(16..=27) {
        let style = style_of_code(code);
        let direction = direction_name(style);
        assert_eq!(choose(Some(kind_name(style)), direction.as_deref(), alignment_name(style), Style::Radial), Ok(style), "{code}");
    }
}
