//! A menu, a split button, a gallery or a box inside the card of a folded ribbon group.
//!
//! When the window is narrow a group folds into one button, and the button opens a card with the group's controls. The menus of those controls must open
//! over the card: the card and the menu were both asked of egui's one slot for an open popup, so the menu pushed the card out and neither showed. The tests
//! give the window pointer and keys as a person's would be, at narrow widths where the groups named are folded (each test says so first, so a layout that
//! moves fails loudly and does not pass by testing the unfolded group), and check the drawing the commands changed and which controls the window drew.

use diacraft_ui_egui::ribbon::spec::{Kind, TabContext, visible_tabs};
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{frame_of, key, shape_ids, type_text};
use crate::operations::{developer_window, two_squares, undo_steps};
use crate::support::{Rest, TempDir, Window, click, elements_named, run_command};

const FOLD_SHAPE_DESIGN: &str = "ribbon.Developer.fold.GroupShapeDesign";
const FOLD_ARRANGE: &str = "ribbon.Home.fold.GroupArrange";
const FOLD_CLIPBOARD: &str = "ribbon.Home.fold.GroupClipboard";

/// What a menu, a palette or a list draws when it is open: the names the window gives the rows and boxes of the menus it opens.
const MENU_PARTS: [&str; 4] = ["menu.", "palette.", "font.", "size."];

fn drawn(w: &Window, name: &str) -> bool {
    w.state().element(name).is_some()
}

/// How many parts of an open menu, palette or list the last frame drew.
fn menu_parts(w: &Window) -> usize {
    MENU_PARTS.iter().map(|prefix| elements_named(w, prefix).len()).sum()
}

/// A click on the control called `name` in the last frame's list, then the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn escape(w: &mut Window) {
    key(w, Key::Escape, Modifiers::NONE);
}

/// A window of `width` on the two squares, selected, with the tab `tab` open.
fn window_on(dir: &TempDir, tab: &str, width: f32) -> Window {
    let mut w = developer_window(dir, &two_squares(), [width, 700.0]);
    run_command(&mut w, "window.tab", json!({"tab": tab}));
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    w
}

fn left_of(w: &Window, id: u32) -> f64 {
    frame_of(w, id).0
}

/// The boxes of the two squares: lower left, then size.
fn frames_of(w: &Window) -> [(f64, f64, f64, f64); 2] {
    [frame_of(w, 1), frame_of(w, 2)]
}

// ---- Developer, Shape Design ------------------------------------------------------------------------------------------------------------------

/// Operations opens from the card of the folded Shape Design group and Union runs from it: the menu is over the card, the entry runs its command as one undo
/// step, and the menu and the card both close.
#[test]
fn the_operations_menu_opens_from_the_folded_shape_design_card_and_union_runs() {
    for width in [480.0_f32, 520.0] {
        let dir = TempDir::new("fold-ops");
        let mut w = window_on(&dir, "Developer", width);
        assert!(drawn(&w, FOLD_SHAPE_DESIGN) && !drawn(&w, "ribbon.Developer.OperationsMenu"), "Shape Design is folded at {width}");
        hit(&mut w, FOLD_SHAPE_DESIGN);
        assert!(w.state().element("ribbon.Developer.OperationsMenu").is_some_and(|e| e.enabled), "the card has Operations, working, at {width}");
        assert_eq!(menu_parts(&w), 0, "no menu is open yet");

        hit(&mut w, "ribbon.Developer.OperationsMenu");
        assert!(drawn(&w, "menu.ShapesObjectUnion"), "the menu opened at {width}");
        assert!(w.state().element("menu.ShapesObjectUnion").is_some_and(|e| e.enabled), "two shapes are selected: Union works at {width}");
        assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "the card is still under its menu at {width}");
        let rows = elements_named(&w, "menu.ShapesObject");
        assert_eq!(rows.len(), 9, "the nine rows at {width}");
        for (name, rect, _) in &rows {
            assert!(rect.min.x >= 0.0 && rect.max.x <= width + 0.5, "{name} is inside the window at {width}: {rect:?}");
        }

        let steps = undo_steps(&w);
        hit(&mut w, "menu.ShapesObjectUnion");
        assert!(w.state().notice().is_none(), "{:?}", w.state().notice());
        assert_eq!(shape_ids(&w).len(), 1, "Union made one shape at {width}");
        assert_eq!(undo_steps(&w), (steps.0 + 1, 0), "one undo step at {width}");
        assert_eq!(menu_parts(&w), 0, "the menu closed at {width}");
        assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "and so did the card, at {width}");
        assert!(drawn(&w, FOLD_SHAPE_DESIGN), "its button is where it was at {width}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// A click on an entry that cannot run (Join is not built) runs nothing and closes the menu, as any click inside a menu does, and leaves the card to work in; the
/// menu then opens again and Fragment runs from it.
#[test]
fn a_quiet_entry_in_the_menu_of_a_card_runs_nothing_and_leaves_the_card() {
    let dir = TempDir::new("fold-ops-quiet");
    let mut w = window_on(&dir, "Developer", 480.0);
    hit(&mut w, FOLD_SHAPE_DESIGN);
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    assert!(w.state().element("menu.ShapesObjectJoin").is_some_and(|e| !e.enabled), "Join is not built: it is quiet");
    let revision = w.state().session().revision();
    hit(&mut w, "menu.ShapesObjectJoin");
    assert_eq!(w.state().session().revision(), revision, "a quiet row runs nothing");
    assert_eq!(menu_parts(&w), 0, "the click closed the menu");
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "and the card is there to open it again");
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    hit(&mut w, "menu.ShapesObjectFragment");
    assert_eq!(shape_ids(&w).len(), 3, "Fragment ran from the second opening");
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "and closed the card");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Home, Arrange ----------------------------------------------------------------------------------------------------------------------------

/// The Align menu opens from the card of the folded Arrange group and an entry moves the shapes: the second shape goes to the first one's left edge.
#[test]
fn the_align_menu_opens_from_the_folded_arrange_card_and_an_entry_runs() {
    // Measured caption spacing can keep Arrange compact at 800 points. These widths deliberately
    // exercise its folded card; the assertion below must fail if a future layout stops folding it.
    for width in [480.0_f32, 560.0, 640.0] {
        let dir = TempDir::new("fold-align");
        let mut w = window_on(&dir, "Home", width);
        assert!(drawn(&w, FOLD_ARRANGE) && !drawn(&w, "ribbon.Home.AlignGallery"), "Arrange is folded at {width}");
        assert_eq!((left_of(&w, 1), left_of(&w, 2)), (3.0, 4.0));
        hit(&mut w, FOLD_ARRANGE);
        assert!(w.state().element("ribbon.Home.AlignGallery").is_some_and(|e| e.enabled), "the card has Align, working, at {width}");

        hit(&mut w, "ribbon.Home.AlignGallery");
        for row in ["AlignShapesLeft", "AlignShapesCenter", "AlignShapesRight", "AlignShapesTop", "AlignShapesMiddle", "AlignShapesBottom"] {
            assert!(w.state().element(&format!("menu.{row}")).is_some_and(|e| e.enabled), "{row} is in the menu and works at {width}");
        }
        assert!(drawn(&w, "ribbon.Home.AlignGallery"), "the card is under the menu at {width}");

        let steps = undo_steps(&w);
        hit(&mut w, "menu.AlignShapesLeft");
        assert!(w.state().notice().is_none(), "{:?}", w.state().notice());
        assert_eq!((left_of(&w, 1), left_of(&w, 2)), (3.0, 3.0), "the others went to the first shape's left edge at {width}");
        assert_eq!(undo_steps(&w).0, steps.0 + 1, "one undo step at {width}");
        assert_eq!(menu_parts(&w), 0, "the menu closed at {width}");
        assert!(!drawn(&w, "ribbon.Home.AlignGallery"), "and the card, at {width}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

// ---- a split button -------------------------------------------------------------------------------------------------------------------------

/// Paste is a split button: its icon runs Paste and the label with its arrow under the icon opens a menu. In the card of the folded Clipboard group the arrow
/// opens the menu over the card and Paste Duplicate runs from it (and closes both); the icon runs its command and closes the card too (a command
/// run from a card closes it, so the icon and the entry of one split button agree).
#[test]
fn a_split_button_in_a_folded_card_opens_its_menu_and_its_icon_runs_its_command() {
    let dir = TempDir::new("fold-split");
    let mut w = window_on(&dir, "Home", 400.0);
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    assert!(drawn(&w, FOLD_CLIPBOARD) && !drawn(&w, "ribbon.Home.PasteMenu"), "Clipboard is folded at 400");

    // The arrow: the menu.
    hit(&mut w, FOLD_CLIPBOARD);
    assert!(drawn(&w, "ribbon.Home.PasteMenu.menu"), "the card has the split button");
    assert!(w.state().element("ribbon.Home.PasteMenu.menu").is_some_and(|e| e.enabled), "its menu part works: Paste Duplicate can run on a shape");
    hit(&mut w, "ribbon.Home.PasteMenu.menu");
    assert!(w.state().element("menu.PasteDuplicate").is_some_and(|e| e.enabled), "the menu is open over the card");
    assert!(drawn(&w, "ribbon.Home.PasteMenu"), "the card is under it");
    hit(&mut w, "menu.PasteDuplicate");
    assert_eq!(shape_ids(&w).len(), 3, "Paste Duplicate ran");
    assert_eq!(menu_parts(&w), 0);
    assert!(!drawn(&w, "ribbon.Home.PasteMenu"), "an entry closed the menu and the card");

    // The icon: the command. A copy first, so that Paste has something to paste.
    run_command(&mut w, "clipboard.copy", json!({}));
    hit(&mut w, FOLD_CLIPBOARD);
    assert!(w.state().element("ribbon.Home.PasteMenu.main").is_some_and(|e| e.enabled), "Paste works after a copy");
    hit(&mut w, "ribbon.Home.PasteMenu.main");
    assert_eq!(shape_ids(&w).len(), 4, "Paste ran from the card");
    assert!(!drawn(&w, "ribbon.Home.PasteMenu"), "a plain button run from the card closed it, as a menu entry does");
    assert_eq!(w.state().ui_panics(), 0);
}

/// And 75b: a plain button, a tool picked and a toggle (Bold) run from a card close it, as an entry of a menu does; **a check box ticked keeps it
/// open**, so that several can be ticked in a row (Ruler, then Grid). A tool picked from a card does not leave the card over the drawing, and the first Escape
/// after it is the drawing's (it leaves the tool).
#[test]
fn a_plain_button_a_tool_and_a_toggle_run_from_a_card_close_it_and_a_check_box_does_not() {
    // A tool: Connector, from the folded Tools group.
    let dir = TempDir::new("fold-plain-tool");
    let mut w = window_on(&dir, "Home", 440.0);
    let tools = "ribbon.Home.fold.GroupTools";
    assert!(drawn(&w, tools) && !drawn(&w, "ribbon.Home.ConnectorTool"), "Tools is folded at 440");
    hit(&mut w, tools);
    assert!(drawn(&w, "ribbon.Home.ConnectorTool"));
    hit(&mut w, "ribbon.Home.ConnectorTool");
    assert_eq!(w.state().window_state()["tool"], "connector", "the tool is on");
    assert!(!drawn(&w, "ribbon.Home.ConnectorTool"), "and the card is not over the drawing");
    escape(&mut w);
    assert_eq!(w.state().window_state()["tool"], "pointer", "the first Escape was the drawing's: it left the tool");

    // A style button: Bold, from the folded Font group.
    let dir = TempDir::new("fold-plain-bold");
    let mut w = window_on(&dir, "Home", 480.0);
    let font = "ribbon.Home.fold.GroupFont";
    assert!(drawn(&w, font) && !drawn(&w, "ribbon.Home.Bold"), "Font is folded at 480");
    hit(&mut w, font);
    let steps = undo_steps(&w);
    hit(&mut w, "ribbon.Home.Bold");
    assert_eq!(undo_steps(&w).0, steps.0 + 1, "Bold ran: one undo step");
    assert!(!drawn(&w, "ribbon.Home.Bold"), "and closed the card");

    // A check box: Grid and then Ruler, from the folded Show group of the View tab. The card stays for the second.
    let dir = TempDir::new("fold-plain-grid");
    let mut w = window_on(&dir, "View", 480.0);
    let show = "ribbon.View.fold.GroupViewShowHide";
    assert!(drawn(&w, show) && !drawn(&w, "ribbon.View.ShowGrid"), "Show is folded at 480");
    let (grid, rulers) = (w.state().window_state()["grid"].as_bool().unwrap(), w.state().window_state()["rulers"].as_bool().unwrap());
    hit(&mut w, show);
    hit(&mut w, "ribbon.View.ShowGrid");
    assert_eq!(w.state().window_state()["grid"].as_bool().unwrap(), !grid, "the grid went the other way");
    assert!(drawn(&w, "ribbon.View.ShowGrid"), "a check box ticked leaves the card open");
    hit(&mut w, "ribbon.View.ShowRuler");
    assert_eq!(w.state().window_state()["rulers"].as_bool().unwrap(), !rulers, "and the next one is ticked from the same card");
    assert_eq!(w.state().window_state()["grid"].as_bool().unwrap(), !grid, "without undoing the first");
    assert!(drawn(&w, "ribbon.View.ShowGrid"), "and the card is still there");
    // The card is shut by Escape, as any card is.
    escape(&mut w);
    assert!(!drawn(&w, "ribbon.View.ShowGrid"), "Escape closes the card");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Escape and click outside -----------------------------------------------------------------------------------------------------------------

/// Escape closes the menu first and then the card, and neither is the drawing's Escape: the shapes stay selected until the third one.
#[test]
fn escape_closes_the_menu_then_the_card_and_only_then_clears_the_selection() {
    let dir = TempDir::new("fold-escape");
    let mut w = window_on(&dir, "Developer", 480.0);
    hit(&mut w, FOLD_SHAPE_DESIGN);
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    assert!(drawn(&w, "menu.ShapesObjectUnion") && drawn(&w, "ribbon.Developer.OperationsMenu"));

    escape(&mut w);
    assert_eq!(menu_parts(&w), 0, "the first Escape closed the menu");
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "and left the card");
    assert_eq!(w.state().selection(), [1, 2], "and the selection");

    escape(&mut w);
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "the second closed the card");
    assert!(drawn(&w, FOLD_SHAPE_DESIGN));
    assert_eq!(w.state().selection(), [1, 2], "the shapes are still selected: the Escape was the card's");

    escape(&mut w);
    assert!(w.state().selection().is_empty(), "the third is the drawing's: the selection goes");
    assert_eq!(w.state().ui_panics(), 0);
}

/// With the card open and no menu, Escape closes the card and leaves the selection; a second click on the card's button closes it too.
#[test]
fn escape_and_a_second_click_on_the_button_close_a_card_with_no_menu_open() {
    let dir = TempDir::new("fold-escape-card");
    let mut w = window_on(&dir, "Developer", 480.0);
    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"));
    escape(&mut w);
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "Escape closed the card");
    assert_eq!(w.state().selection(), [1, 2], "and left the selection");

    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"));
    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "a second click on the button closed it");
}

/// A click outside closes the menu and the card together, and a click outside closes a card that has no menu open; a click inside the card, on the card itself
/// and not on a menu button, leaves it open.
#[test]
fn a_click_outside_closes_the_menu_and_the_card_together() {
    let dir = TempDir::new("fold-outside");
    let mut w = window_on(&dir, "Developer", 480.0);
    let canvas = w.state().element("canvas").unwrap().rect;
    // Far from the card, which opens under the ribbon over the drawing's top left.
    let outside = canvas.right_bottom() - egui::vec2(30.0, 30.0);

    hit(&mut w, FOLD_SHAPE_DESIGN);
    hit(&mut w, "ribbon.Developer.OperationsMenu");
    assert!(drawn(&w, "menu.ShapesObjectUnion"));
    click(&mut w, outside);
    w.rest();
    assert_eq!(menu_parts(&w), 0, "the menu closed");
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "and the card");

    // The card alone.
    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"));
    click(&mut w, outside);
    w.rest();
    assert!(!drawn(&w, "ribbon.Developer.OperationsMenu"), "a click outside closed the card");

    // A click on the card itself, just above its controls, leaves it open (a control that runs a command closes it).
    hit(&mut w, FOLD_SHAPE_DESIGN);
    let card = w.state().element("ribbon.Developer.OperationsMenu").unwrap().rect;
    click(&mut w, egui::pos2(card.center().x, card.top() - 2.0));
    w.rest();
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "a click inside the card keeps it");
}

/// A click on another folded group's button closes the card that is open and opens that group's.
#[test]
fn a_click_on_another_folded_button_moves_the_card() {
    let dir = TempDir::new("fold-switch");
    let mut w = window_on(&dir, "Home", 480.0);
    let editing = "ribbon.Home.fold.GroupEditing";
    assert!(drawn(&w, FOLD_ARRANGE) && drawn(&w, editing), "Arrange and Editing are folded at 480");
    for (first, second) in [(FOLD_ARRANGE, editing), (editing, FOLD_ARRANGE)] {
        hit(&mut w, first);
        let first_card = if first == FOLD_ARRANGE { "ribbon.Home.AlignGallery" } else { "ribbon.Home.ReplaceShape" };
        let second_card = if first == FOLD_ARRANGE { "ribbon.Home.ReplaceShape" } else { "ribbon.Home.AlignGallery" };
        assert!(drawn(&w, first_card) && !drawn(&w, second_card));
        hit(&mut w, second);
        assert!(drawn(&w, second_card) && !drawn(&w, first_card), "the card moved from {first} to {second}");
        escape(&mut w);
        assert!(!drawn(&w, second_card));
    }
}

/// A card does not outlive its button: another tab, or a window that has room for the group, shows no card the next time the group is folded.
#[test]
fn a_card_open_when_the_tab_changes_is_closed_when_the_tab_comes_back() {
    let dir = TempDir::new("fold-stale");
    let mut w = window_on(&dir, "Developer", 480.0);
    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu"));
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
    run_command(&mut w, "window.tab", json!({"tab": "Developer"}));
    assert!(drawn(&w, FOLD_SHAPE_DESIGN) && !drawn(&w, "ribbon.Developer.OperationsMenu"), "the card did not come back by itself");
    // And Escape is the drawing's again at once.
    escape(&mut w);
    assert!(w.state().selection().is_empty());

    // A window that grows while the card is open: the group unfolds, and when the window narrows again the group folds shut.
    let dir = TempDir::new("fold-resize");
    let mut w = window_on(&dir, "Developer", 480.0);
    hit(&mut w, FOLD_SHAPE_DESIGN);
    assert!(drawn(&w, "ribbon.Developer.OperationsMenu") && drawn(&w, FOLD_SHAPE_DESIGN));
    w.set_size(egui::vec2(1280.0, 700.0));
    w.rest();
    assert!(!drawn(&w, FOLD_SHAPE_DESIGN) && drawn(&w, "ribbon.Developer.OperationsMenu"), "the group is unfolded: its controls are on the ribbon");
    w.set_size(egui::vec2(480.0, 700.0));
    w.rest();
    assert!(drawn(&w, FOLD_SHAPE_DESIGN) && !drawn(&w, "ribbon.Developer.OperationsMenu"), "folded again, and shut");
    escape(&mut w);
    assert!(w.state().selection().is_empty(), "no card is open to take the Escape");
}

// ---- the keyboard -----------------------------------------------------------------------------------------------------------------------------

/// Enter on a focused button opens the menu and the arrow keys and Enter choose a row, in the group as designed and in its card alike.
#[test]
fn the_keys_open_and_choose_from_a_menu_in_a_folded_card_as_they_do_in_the_group() {
    for (width, folded) in [(1280.0_f32, false), (480.0, true)] {
        let dir = TempDir::new("fold-keys");
        let mut w = window_on(&dir, "Developer", width);
        assert_eq!(drawn(&w, FOLD_SHAPE_DESIGN), folded, "Shape Design is folded at {width}: {folded}");
        if folded {
            w.get_by_label("Shape Design").focus();
            w.step();
            key(&mut w, Key::Enter, Modifiers::NONE);
            assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "Enter on the focused button opened the card at {width}");
        }
        w.get_by_label("Operations").focus();
        w.step();
        key(&mut w, Key::Enter, Modifiers::NONE);
        assert!(drawn(&w, "menu.ShapesObjectUnion"), "Enter on the focused Operations opened its menu at {width}");
        let frames = frames_of(&w);
        key(&mut w, Key::ArrowDown, Modifiers::NONE);
        w.rest();
        assert_eq!(frames_of(&w), frames, "the arrow key moved a row's focus and not the shapes at {width}");
        key(&mut w, Key::Enter, Modifiers::NONE);
        assert_eq!(shape_ids(&w).len(), 1, "Enter chose the first row, Union, at {width}");
        assert_eq!(menu_parts(&w), 0, "the menu closed at {width}");
        assert_eq!(
            drawn(&w, "ribbon.Developer.OperationsMenu"),
            !folded,
            "a folded group's card closed with it and an unfolded group stays, at {width}"
        );
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// The drawing's keys stand aside while a menu is open or the card of a folded group is: arrows do not nudge the shapes selected, F2 and a typed letter do not
/// open a text box on them. (An unfolded group's menu had the same fault.) With the menu and the card shut the same keys work: the arrow moves the shapes.
#[test]
fn the_drawings_keys_stand_aside_while_a_menu_or_a_card_is_open() {
    for (width, folded) in [(1280.0_f32, false), (480.0, true)] {
        let dir = TempDir::new("fold-drawing-keys");
        let mut w = window_on(&dir, "Developer", width);
        assert_eq!(drawn(&w, FOLD_SHAPE_DESIGN), folded);
        let before = frames_of(&w);
        let nothing_happens = |w: &mut Window, what: &str| {
            key(w, Key::ArrowRight, Modifiers::NONE);
            key(w, Key::ArrowDown, Modifiers::SHIFT);
            key(w, Key::F2, Modifiers::NONE);
            type_text(w, "a");
            assert_eq!(frames_of(w), before, "{what} at {width}: no shape moved");
            assert_eq!(w.state().window_state()["editingText"], false, "{what} at {width}: no text box opened");
            assert_eq!(w.state().selection(), [1, 2], "{what} at {width}: the selection is as it was");
        };
        if folded {
            // The card, with no menu over it.
            hit(&mut w, FOLD_SHAPE_DESIGN);
            assert!(drawn(&w, "ribbon.Developer.OperationsMenu"));
            nothing_happens(&mut w, "with the card open");
            assert!(drawn(&w, "ribbon.Developer.OperationsMenu"), "the keys left the card open at {width}");
        }
        // A menu: over the card, or in the group.
        hit(&mut w, "ribbon.Developer.OperationsMenu");
        assert!(drawn(&w, "menu.ShapesObjectUnion"));
        nothing_happens(&mut w, "with the menu open");
        // Shut, the arrow is the drawing's again.
        escape(&mut w);
        if folded {
            escape(&mut w);
        }
        assert_eq!(menu_parts(&w), 0);
        key(&mut w, Key::ArrowRight, Modifiers::NONE);
        assert_ne!(frames_of(&w), before, "with nothing open the arrow nudges the shapes at {width}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// A dialog closes the card: the Text dialog opened from the launcher of the folded Font group, and one a command opens while a card is open (the control
/// channel, a key): the card is gone while the dialog is open, and it does not come back when the dialog closes.
#[test]
fn a_dialog_that_opens_closes_the_card_and_it_stays_shut_when_the_dialog_closes() {
    let font = "ribbon.Home.fold.GroupFont";
    // From the launcher.
    let dir = TempDir::new("fold-dialog-launcher");
    let mut w = window_on(&dir, "Home", 480.0);
    assert!(drawn(&w, font) && !drawn(&w, "ribbon.Home.Bold"), "Font is folded at 480");
    hit(&mut w, font);
    assert!(
        drawn(&w, "ribbon.Home.Bold") && w.state().element("ribbon.Home.FontDialog").is_some_and(|e| e.enabled),
        "the card has Bold and the launcher"
    );
    hit(&mut w, "ribbon.Home.FontDialog");
    assert_eq!(w.state().window_state()["textDialog"], "Font", "the launcher opened the Text dialog");
    assert!(!drawn(&w, "ribbon.Home.Bold"), "the card is gone while the dialog is open");
    hit(&mut w, "textdialog.cancel");
    assert!(w.state().window_state()["textDialog"].is_null(), "the dialog closed");
    assert!(!drawn(&w, "ribbon.Home.Bold") && drawn(&w, font), "the card did not come back with it");

    // A dialog that something other than the card opens, with the card open: the window's own check closes it.
    let dir = TempDir::new("fold-dialog-command");
    let mut w = window_on(&dir, "Home", 480.0);
    hit(&mut w, font);
    assert!(drawn(&w, "ribbon.Home.Bold"));
    run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
    assert_eq!(w.state().window_state()["textDialog"], "Font");
    assert!(!drawn(&w, "ribbon.Home.Bold"), "the card closed under the dialog a command opened");
    hit(&mut w, "textdialog.cancel");
    assert!(w.state().window_state()["textDialog"].is_null());
    assert!(!drawn(&w, "ribbon.Home.Bold") && drawn(&w, font), "and stayed shut when the dialog closed");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- every group ------------------------------------------------------------------------------------------------------------------------------

/// **Every group**: each menu, split button, gallery and box (Font, Size) that works, in every group of every tab that folds at a narrow width, opens its menu
/// over the card, Escape closes the menu and leaves the card, and a second Escape closes the card.
#[test]
fn every_menu_of_every_folded_group_opens_over_its_card() {
    let mut opened = Vec::new();
    for width in [320.0_f32, 480.0, 640.0] {
        for tab in visible_tabs(TabContext { developer: true, org_chart: false, ..TabContext::default() }) {
            let dir = TempDir::new("fold-every");
            let mut w = window_on(&dir, tab.label, width);
            for group in tab.groups {
                let fold = format!("ribbon.{}.fold.{}", tab.label, group.id);
                // A folded button the narrow ribbon scrolls off the window cannot be clicked.
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 700.0));
                if !w.state().element(&fold).is_some_and(|e| screen.contains_rect(e.rect)) {
                    continue;
                }
                for ctl in group.top_level() {
                    let opens_a_menu = matches!(ctl.kind, Kind::Menu | Kind::Split | Kind::Gallery | Kind::Combo);
                    if !opens_a_menu || (ctl.items.is_empty() && ctl.palette.is_none() && ctl.kind != Kind::Combo) {
                        continue;
                    }
                    hit(&mut w, &fold);
                    let name = format!("ribbon.{}.{}", tab.label, ctl.id);
                    // A split button opens its menu from its arrow part, and a gallery from the arrow at the end of its strip (a press on a tile applies it).
                    let target =
                        [format!("{name}.menu"), format!("{name}.more")].into_iter().find(|part| drawn(&w, part)).unwrap_or_else(|| name.clone());
                    let what = format!("{} > {} > {} at {width}", tab.label, group.label, ctl.id);
                    let Some(element) = w.state().element(&target).cloned() else {
                        panic!("the card of {what} does not draw the control");
                    };
                    if !element.enabled || !screen.contains_rect(element.rect) {
                        // A control that cannot work now is quiet, and one a card wider than the window pushes off it cannot be clicked.
                        escape(&mut w);
                        continue;
                    }
                    assert_eq!(menu_parts(&w), 0, "{what}: nothing is open before the click");
                    hit(&mut w, &target);
                    assert!(menu_parts(&w) > 0, "{what}: the click opened no menu");
                    assert!(drawn(&w, &target), "{what}: the card is under the menu");
                    escape(&mut w);
                    assert_eq!(menu_parts(&w), 0, "{what}: Escape closed the menu");
                    assert!(drawn(&w, &target), "{what}: and left the card");
                    escape(&mut w);
                    assert!(!drawn(&w, &target), "{what}: a second Escape closed the card");
                    opened.push(what);
                }
            }
            assert_eq!(w.state().ui_panics(), 0, "{} at {width}", tab.label);
        }
    }
    // A test that opened nothing proves nothing, and these are the menus the owner found shut: each was tried where its group folds.
    for must in [
        "Developer > Shape Design > OperationsMenu at 480",
        "Home > Arrange > AlignGallery at 480",
        "Home > Arrange > AlignGallery at 640",
        "Home > Clipboard > PasteMenu at 320",
        "Home > Font > Font at 480",
        "Home > Font > FontSize at 480",
        "Home > Font > FontColorGallery at 480",
        "Home > Shape Styles > FillColorGallery at 480",
        "Design > Page Setup > PageSize at 320",
    ] {
        assert!(opened.iter().any(|what| what == must), "{must} was not tried: {opened:?}");
    }
    assert!(opened.len() >= 30, "only {} menus were tried: {opened:?}", opened.len());
}
