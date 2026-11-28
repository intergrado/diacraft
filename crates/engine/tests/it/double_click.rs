//! What a double click on a shape does as far as its `EventDblClick` says (OPENGROUPWIN).
//!
//! No group window exists, so a group whose cell calls `OPENGROUPWIN()` is told so and a shape that is no group is left alone; every other cell is the default. The window's
//! own tests (`crates/ui-egui/tests/it/group_text.rs`) hold what the person sees; these hold the reading of the cell.

use diacraft_engine::doc::Document;
use diacraft_engine::draw::double_click::{DoubleClick, NO_GROUP_WINDOWS, on_double_click};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

fn page_with(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn with_event(shape: ShapeBuilder, formula: &str) -> ShapeBuilder {
    shape.formula("EventDblClick", formula, "0")
}

#[test]
fn a_group_that_calls_open_group_window_gets_the_status_bar_and_a_shape_that_is_no_group_gets_nothing() {
    for formula in
        ["OPENGROUPWIN()", "_OPENGROUPWIN()", "openGroupWin()", "IF(1,OPENGROUPWIN(),OPENTEXTWIN())", "1+SUM(2,OPENGROUPWIN())", "-OPENGROUPWIN()"]
    {
        let group = page_with(with_event(ShapeBuilder::new(1).group(), formula));
        assert_eq!(on_double_click(&group, 1, 1), DoubleClick::GroupWindow, "{formula}");
        let plain = page_with(with_event(ShapeBuilder::new(1), formula));
        assert_eq!(on_double_click(&plain, 1, 1), DoubleClick::Nothing, "{formula}");
    }
}

#[test]
fn any_other_cell_is_the_default_and_edits_the_text() {
    // A call of something else, a name that only holds the word, a call with a qualifier (another sheet's function), a formula that does not parse, no cell, and no shape.
    for formula in ["OPENTEXTWIN()", "DEFAULTEVENT()", "0", "OPENGROUPWINDOW()", "Sheet.5!OPENGROUPWIN()", "OPENGROUPWIN(", "User.OPENGROUPWIN"] {
        let doc = page_with(with_event(ShapeBuilder::new(1).group(), formula));
        assert_eq!(on_double_click(&doc, 1, 1), DoubleClick::EditText, "{formula}");
    }
    let none = page_with(ShapeBuilder::new(1).group());
    assert_eq!(on_double_click(&none, 1, 1), DoubleClick::EditText, "no cell");
    assert_eq!(on_double_click(&none, 1, 99), DoubleClick::EditText, "no such shape");
    assert_eq!(on_double_click(&none, 7, 1), DoubleClick::EditText, "no such page");
}

/// A cell that a master holds is the instance's (`EventDblClick` is found by inheritance), as 132 master shapes of the files hold it: the group made from the master is told.
#[test]
fn the_cell_is_read_through_the_master() {
    let master = MasterBuilder::new().shape(with_event(ShapeBuilder::new(1).group(), "OPENGROUPWIN()"));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).group())).build();
    assert_eq!(on_double_click(&doc, 1, 10), DoubleClick::GroupWindow);
}

/// The formula is walked with a stack of its own: a chain as deep as the parser lets a tree be (`diacraft_formula::MAX_DEPTH`, 512) that ends in the call is found, a chain that
/// has no call is not, and a chain past the depth does not parse and is the default.
#[test]
fn a_chain_as_deep_as_a_formula_goes_is_walked_and_a_deeper_one_is_the_default() {
    let chain = |terms: usize, last: &str| format!("{}{last}", "1+".repeat(terms));
    let found = page_with(with_event(ShapeBuilder::new(1).group(), &chain(400, "OPENGROUPWIN()")));
    assert_eq!(on_double_click(&found, 1, 1), DoubleClick::GroupWindow);
    let missed = page_with(with_event(ShapeBuilder::new(1).group(), &chain(400, "OPENTEXTWIN()")));
    assert_eq!(on_double_click(&missed, 1, 1), DoubleClick::EditText);
    let too_deep = page_with(with_event(ShapeBuilder::new(1).group(), &chain(4_000, "OPENGROUPWIN()")));
    assert_eq!(on_double_click(&too_deep, 1, 1), DoubleClick::EditText, "a formula past the depth does not parse, and an unparsed cell says nothing");
}

#[test]
fn the_words_say_there_are_no_group_windows() {
    assert!(NO_GROUP_WINDOWS.contains("no group windows"), "{NO_GROUP_WINDOWS}");
    assert!(NO_GROUP_WINDOWS.contains("group window"), "{NO_GROUP_WINDOWS}");
}
