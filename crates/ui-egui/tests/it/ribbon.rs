//! The tab row and the ribbon: the order of the tabs and of the groups on each tab (test vectors),
//! what is enabled and what is not, and how the ribbon fits a window of any width.

use diacraft_ui_egui::commands::known;
use diacraft_ui_egui::ribbon::spec::{self, TabContext, all_tabs, tab_names, visible_tabs};
use egui::Rect;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::support::{drawing_window, elements_named, run_command};

fn default_names() -> Vec<&'static str> {
    tab_names(TabContext::default())
}

/// V2 (window-layout): the tabs of a default window, in order.
#[test]
fn the_tabs_of_a_default_window_are_in_the_spec_order() {
    assert_eq!(default_names(), ["File", "Home", "Insert", "Draw", "Design", "Data", "Process", "Review", "View", "Help"]);
}

/// Developer between View and Help once it is on; Org Chart after Developer, if on, else after View, before Help.
#[test]
fn developer_and_org_chart_take_their_places() {
    let names = |developer, org_chart| tab_names(TabContext { developer, org_chart, ..TabContext::default() });
    assert_eq!(names(true, false), ["File", "Home", "Insert", "Draw", "Design", "Data", "Process", "Review", "View", "Developer", "Help"]);
    assert_eq!(names(false, true), ["File", "Home", "Insert", "Draw", "Design", "Data", "Process", "Review", "View", "Org Chart", "Help"]);
    assert_eq!(names(true, true)[9..], ["Developer", "Org Chart", "Help"]);
}

/// The drawn row, not only the table: the tab buttons stand in the same order from left to right, File first.
#[test]
fn the_drawn_tab_row_follows_the_table() {
    let window = drawing_window([1280.0, 720.0]);
    let mut drawn: Vec<(String, Rect, bool)> = elements_named(&window, "tab.");
    drawn.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    let labels: Vec<&str> = drawn.iter().map(|(n, ..)| n.strip_prefix("tab.").unwrap_or(n)).collect();
    assert_eq!(labels, default_names());
    assert!(drawn.iter().all(|(_, _, enabled)| *enabled), "every tab can be opened");
    // On one row.
    let y = drawn[0].1.center().y;
    assert!(drawn.iter().all(|(_, r, _)| (r.center().y - y).abs() < 1.0));
}

/// A search box in the title bar and none on the tab row; Comments, Share (disabled) at the right end of the row.
#[test]
fn search_is_in_the_title_bar_and_comments_and_share_end_the_tab_row() {
    let window = drawing_window([1280.0, 720.0]);
    let search = window.state().element("titlebar.search").expect("a search box in the title bar").clone();
    let tab_y = window.state().element("tab.Home").unwrap().rect.center().y;
    assert!(search.rect.center().y < tab_y, "the search box is above the tab row");
    assert!(!search.enabled, "it does nothing yet, so it is drawn quiet");
    let last_tab = window.state().element("tab.Help").unwrap().rect;
    let (comments, share) = (window.state().element("tab_end.Comments").unwrap().clone(), window.state().element("tab_end.Share").unwrap().clone());
    assert!(last_tab.right() < comments.rect.left() && comments.rect.right() <= share.rect.left(), "Comments, then Share, after the tabs");
    assert!(!comments.enabled && !share.enabled);
    // And on the same row as the tabs.
    assert!((comments.rect.center().y - tab_y).abs() < 6.0);
}

/// The labels of the groups the window draws on `tab`, left to right.
fn drawn_groups(window: &crate::support::Window, tab: &str) -> Vec<String> {
    let prefix = format!("ribbon.{tab}.group.");
    let mut found = elements_named(window, &prefix);
    found.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    found.into_iter().map(|(n, ..)| n.strip_prefix(&prefix).unwrap_or(&n).to_owned()).collect()
}

/// The groups of each tab, in order, as the window draws them. Wide, so that none is folded.
#[test]
fn each_tab_draws_its_groups_in_the_spec_order() {
    let vectors: [(&str, &[&str]); 8] = [
        ("Home", &["Clipboard", "Font", "Paragraph", "Tools", "Shape Styles", "Arrange", "Editing"]),
        ("Insert", &["Pages", "Illustrations", "Diagram Parts", "Links", "Text"]),
        ("Design", &["Page Setup", "Themes", "Variants", "Backgrounds", "Layout"]),
        ("Data", &["Create from Data", "External Data", "Data Graphics", "Display Data", "Show/Hide", "Advanced Data Linking"]),
        ("Review", &["Proofing", "Accessibility", "Language", "Comments", "Reports"]),
        ("View", &["Export", "Views", "Show", "Zoom", "Visual Aids", "Window", "Macros"]),
        ("Process", &["Subprocess", "Diagram Validation", "SharePoint Workflow", "Export"]),
        ("Help", &["Help"]),
    ];
    let mut window = drawing_window([2400.0, 800.0]);
    for (tab, groups) in vectors {
        run_command(&mut window, "window.tab", json!({ "tab": tab }));
        assert_eq!(drawn_groups(&window, tab), groups, "the {tab} tab");
    }
    // Developer is off until turned on, then has its groups.
    assert!(window.state().element("tab.Developer").is_none());
    run_command(&mut window, "window.developer", json!({ "on": true }));
    run_command(&mut window, "window.tab", json!({ "tab": "Developer" }));
    assert_eq!(drawn_groups(&window, "Developer"), ["Code", "Add-ins", "Controls", "Shape Design", "Stencil", "Show/Hide", "Tools"]);
}

/// Exactly one group labelled Themes with Variants next to it, and no Insights group.
#[test]
fn design_has_one_themes_group_and_review_has_no_insights_group() {
    let design = spec::tab_by_label("Design").unwrap();
    assert_eq!(design.groups.iter().filter(|g| g.label == "Themes").count(), 1);
    let review = spec::tab_by_label("Review").unwrap();
    assert!(!review.groups.iter().any(|g| g.label == "Insights"));
}

/// The tabs' tables and the map agree on the first and the last tab of the core list (ribbon map V1: Home first).
#[test]
fn the_tables_start_at_home_and_every_group_has_controls() {
    assert_eq!(visible_tabs(TabContext::default())[0].label, "Home");
    for tab in all_tabs() {
        for group in tab.groups {
            assert!(!group.top_level().is_empty(), "{} / {} is empty", tab.label, group.label);
            assert!(!group.label.is_empty());
        }
    }
}

/// Every control that names a command names one that exists, in the engine or in the window.
#[test]
fn every_control_that_names_a_command_names_a_registered_one() {
    let mut named = 0;
    for tab in all_tabs() {
        for group in tab.groups {
            group.walk(&mut |c| {
                if let Some(command) = c.command {
                    named += 1;
                    assert!(known(command), "{} / {}: `{command}` is not a registered command", tab.label, c.id);
                }
            });
        }
    }
    assert!(named >= 5, "the ribbon names {named} commands: the walk found too few");
}

/// Only what works is enabled: a control is enabled exactly when a command it names can run now (or a menu holds one that can),
/// and every other control is drawn disabled, with no click to take.
#[test]
fn a_control_is_enabled_only_if_a_command_is_wired_to_it() {
    let mut window = drawing_window([2400.0, 800.0]);
    let mut enabled_seen = Vec::new();
    // Developer, Org Chart, Container Format and Cross-Functional Flowchart are not on the row until their object is there (the last two are tested in `container.rs` and `swimlane.rs`).
    for tab in all_tabs().into_iter().filter(|t| !matches!(t.label, "Developer" | "Org Chart" | "Container Format" | "Cross-Functional Flowchart")) {
        run_command(&mut window, "window.tab", json!({ "tab": tab.label }));
        for group in tab.groups {
            let mut tops = group.top_level();
            tops.extend(group.launcher.iter());
            for ctl in tops {
                let name = format!("ribbon.{}.{}", tab.label, ctl.id);
                let Some(drawn) = window.state().element(&name) else { panic!("{name} was not drawn") };
                let wired = |c: &spec::Ctl| {
                    let mut any = false;
                    c.walk(&mut |i| any |= i.command.is_some_and(|cmd| window.state().command_enabled(cmd).is_ok()));
                    any
                };
                assert_eq!(drawn.enabled, wired(ctl), "{name}: enabled {} but wired {}", drawn.enabled, wired(ctl));
                if drawn.enabled {
                    enabled_seen.push(ctl.id);
                }
            }
        }
    }
    // The controls that work today: Pointer Tool, Connector, Text and Text Block, the Drawing Tools menu, Select (it holds Select All), Ruler, Grid, Guides,
    // Task Panes (it holds Shapes), Fit to Window, Page Width, New Page (it holds Blank Page), Text Box (it holds the Text tool), and the Design tab's
    // Orientation, Size, Auto Size and the launchers of Page Setup, the launcher of Shape Styles, which opens the Format Shape pane, and
    // the View tab's Dynamic Grid, Connection Points and the launcher of Visual Aids, the Snap & Glue dialog, and its Snap and Snap to Grid boxes
    // and the Zoom dialog's button, and the Design tab's Themes gallery and Connectors menu (Variants needs a theme on the page, which a
    // new drawing has not), the Draw tab's Lasso Select the Home tab's Find menu (it holds Find and Replace) and the Insert tab's Pictures menu (Link, Field and Symbol need a shape selected) and its Callout gallery.
    for id in [
        "PointerTool",
        "ConnectorTool",
        "TextTool",
        "TextBlockTool",
        "DrawingToolsMenu",
        "SelectMenu",
        "ShowRuler",
        "ShowGrid",
        "ShowGuides",
        "TaskPanesMenu",
        "ZoomFitToWindow",
        "ZoomPageWidth",
        "BlankPageInsertMenu",
        "TextBoxInsertMenu",
        "PageOrientation",
        "PageSize",
        "AutoSizePage",
        "PageSetupDialog",
        "PageSetupPageDialog",
        "ObjectFormatDialog",
        "DynamicGrid",
        "ConnectionPoints",
        "AutoConnect",
        "SnapAndGlueDialog",
        "SnapToggle",
        "SnapToGrid",
        "ZoomDialog",
        "ThemesGallery",
        "ConnectorsGallery",
        "LassoSelect",
        "FindMenu",
        "LayersMenu",
        "FlyoutAnchorInsertPictures",
        // The Insert tab's Container gallery adds a container wherever there is a drawing.
        "ContainerGallery",
        "CalloutGallery",
        // The Data tab's Shape Data Window box opens the pane wherever there is a drawing.
        "ShapeDataWindow",
        // The Review tab's Comments group: New Comment and the menu of the Comments pane, Show Markup, Previous and Next.
        "ReviewNewComment",
        "ShowCommentsMenu",
        // View, Show, Page Breaks is a box of the drawing; the Position menu and Re-Layout Page wait for a shape.
        "ShowPageBreaks",
    ] {
        assert!(enabled_seen.contains(&id), "{id} should work: {enabled_seen:?}");
    }
    assert_eq!(enabled_seen.len(), 39, "{enabled_seen:?}");
}

/// A disabled control says so to assistive technology too, and an enabled one does not.
#[test]
fn disabled_controls_are_disabled_in_the_accessibility_tree() {
    let mut window = drawing_window([2400.0, 800.0]);
    let disabled = |window: &crate::support::Window, label: &str| window.get_by_label(label).accesskit_node().is_disabled();
    assert!(disabled(&window, "Cut"), "Cut does nothing yet");
    assert!(disabled(&window, "Paste"));
    run_command(&mut window, "window.tab", json!({ "tab": "View" }));
    assert!(!disabled(&window, "Grid"));
    assert!(!disabled(&window, "Ruler"));
    assert!(!disabled(&window, "Page Breaks"), "the drawing's box");
    assert!(!disabled(&window, "Connection Points"), "a window setting");
    assert!(!disabled(&window, "AutoConnect"), "the drawing's box");
}

/// Clicking an enabled control runs its command and the window shows the result.
#[test]
fn clicking_the_enabled_controls_runs_their_commands() {
    let mut window = drawing_window([2400.0, 800.0]);
    run_command(&mut window, "window.tab", json!({ "tab": "View" }));
    assert!(window.state().canvas_report().grid_lines.0 > 0 && window.state().canvas_report().rulers);
    window.get_by_label("Grid").click();
    window.step();
    window.step();
    assert_eq!(window.state().canvas_report().grid_lines, (0, 0), "the grid check box turned the grid off");
    window.get_by_label("Ruler").click();
    window.step();
    window.step();
    assert!(!window.state().canvas_report().rulers, "and the ruler check box turned the rulers off");
    window.get_by_label("Grid").click();
    window.step();
    window.step();
    assert!(window.state().canvas_report().grid_lines.0 > 0, "and on again");

    // Zoom in, then Fit to Window brings the page back to fit.
    run_command(&mut window, "view.zoom", json!({ "percent": 200 }));
    window.get_by_label("Fit to Window").click();
    window.step();
    window.step();
    let area = window.state().element("canvas").unwrap().rect;
    let page = window.state().canvas_report().page.unwrap();
    assert!(area.contains_rect(page), "{page:?} is not inside {area:?} after Fit");

    // Insert, New Page, Blank Page.
    run_command(&mut window, "window.tab", json!({ "tab": "Insert" }));
    window.get_by_label("New Page").click();
    window.step();
    window.step();
    window.get_by_label("Blank Page").click();
    window.step();
    window.step();
    assert_eq!(window.state().session().document().unwrap().pages.len(), 2);
}

/// A tab opens by a click, and a double click folds the ribbon to the tab row alone and another unfolds it.
#[test]
fn a_tab_opens_by_a_click_and_the_ribbon_folds_to_the_tab_row() {
    let mut window = drawing_window([1280.0, 720.0]);
    window.get_by_label("View").click();
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["tab"], "View");
    assert!(window.state().element("ribbon.View.ZoomFitToWindow").is_some());
    let canvas_before = window.state().element("canvas").unwrap().rect;

    run_command(&mut window, "window.ribbon", json!({ "collapsed": true }));
    assert!(window.state().element("ribbon.View.ZoomFitToWindow").is_none(), "no ribbon body while folded");
    assert!(window.state().element("tab.View").is_some(), "the tab row stays");
    assert!(window.state().element("canvas").unwrap().rect.height() > canvas_before.height() + 50.0, "the freed height goes to the drawing area");

    // A click on a tab unfolds it.
    window.get_by_label("Insert").click();
    window.step();
    window.step();
    assert!(!window.state().window_state()["ribbonCollapsed"].as_bool().unwrap());
    assert!(window.state().element("ribbon.Insert.BlankPageInsertMenu").is_some());
}

/// The ribbon never clips a group: at every width, every group of every tab is on screen, folded if it must be.
#[test]
fn the_ribbon_folds_groups_rather_than_clipping_them() {
    for width in [480.0, 640.0, 800.0, 1024.0, 1280.0, 1920.0] {
        let mut window = drawing_window([width, 720.0]);
        for tab in
            all_tabs().into_iter().filter(|t| !matches!(t.label, "Developer" | "Org Chart" | "Container Format" | "Cross-Functional Flowchart"))
        {
            run_command(&mut window, "window.tab", json!({ "tab": tab.label }));
            let prefix = format!("ribbon.{}.group.", tab.label);
            let groups = elements_named(&window, &prefix);
            assert_eq!(groups.len(), tab.groups.len(), "{} at {width}: a group is missing", tab.label);
            for (name, rect, _) in groups {
                assert!(rect.min.x >= 0.0 && rect.max.x <= width + 0.5, "{name} at {width} leaves the window: {rect:?}");
            }
        }
    }
}

/// Measured captions reserve a separate region for their dialog launcher, including compact groups.
#[test]
fn group_captions_leave_room_for_their_launchers_at_narrow_widths() {
    for width in [480.0, 540.0, 640.0, 800.0, 1280.0] {
        let mut window = drawing_window([width, 720.0]);
        for tab in visible_tabs(TabContext::default()) {
            run_command(&mut window, "window.tab", json!({ "tab": tab.label }));
            for group in tab.groups {
                let Some(launcher) = group.launcher else { continue };
                let Some(caption) = window.state().element(&format!("ribbon.{}.caption.{}", tab.label, group.id)) else {
                    continue; // A folded group has its caption in the folded button and no launcher.
                };
                let button = window.state().element(&format!("ribbon.{}.{}", tab.label, launcher.id)).unwrap();
                assert!(
                    caption.rect.right() + 3.0 <= button.rect.left(),
                    "{} at {width}: caption {:?}, launcher {:?}",
                    group.label,
                    caption.rect,
                    button.rect
                );
            }
        }
    }
}

/// A group folded to one button opens the whole group, and its controls are there.
#[test]
fn a_folded_group_opens_into_the_whole_group() {
    // Home is the widest tab: at the smallest window the groups at its right end fold.
    let mut window = drawing_window([480.0, 720.0]);
    let mut folds = elements_named(&window, "ribbon.Home.fold.");
    folds.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    assert!(!folds.is_empty(), "a 480 point window folds some groups of Home");
    // Folding goes from the right, so the last group (Editing) is folded.
    let (name, rect, enabled) = folds.last().cloned().unwrap();
    assert!(name.ends_with("GroupEditing") && enabled, "{name}");
    assert!(window.state().element("ribbon.Home.ReplaceShape").is_none(), "folded, its controls are not on the ribbon");
    crate::support::click(&mut window, rect.center());
    window.step();
    window.step();
    let drawn: Vec<String> = elements_named(&window, "ribbon.Home.").into_iter().map(|e| e.0).collect();
    assert!(
        drawn.iter().any(|n| n == "ribbon.Home.ReplaceShape") && drawn.iter().any(|n| n == "ribbon.Home.SelectMenu"),
        "the opened group draws its controls: {drawn:?}"
    );
}

/// The window's text stays inside it and the bars do not overlap at narrow and wide sizes, with the File view open too.
#[test]
fn the_title_bar_and_status_bar_hold_at_narrow_and_wide_sizes() {
    for size in [[480.0, 320.0], [640.0, 400.0], [1280.0, 720.0], [3000.0, 900.0]] {
        let window = drawing_window(size);
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::from(size));
        for name in ["titlebar.name", "qat.Undo", "tab.File", "status.page", "status.fit", "page-tabs.add", "canvas"] {
            let e = window.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn at {size:?}"));
            assert!(screen.contains_rect(e.rect), "{name} leaves the {size:?} window: {:?}", e.rect);
        }
        let title = window.state().element("titlebar.name").unwrap().rect;
        let undo = window.state().element("qat.Undo").unwrap().rect;
        assert!(!title.intersects(undo), "the name overlaps the toolbar at {size:?}");
        if let Some(search) = window.state().element("titlebar.search") {
            assert!(!title.intersects(search.rect), "the name overlaps the search box at {size:?}");
        }
    }
}

/// A row too short for its tabs puts the ones that do not fit in a menu, and none is clipped.
#[test]
fn a_narrow_tab_row_puts_the_tabs_that_do_not_fit_in_a_menu() {
    let wide = drawing_window([1280.0, 720.0]);
    assert!(wide.state().element("tab.overflow").is_none(), "a wide row needs no menu");
    for width in [480.0, 560.0] {
        let mut window = drawing_window([width, 600.0]);
        let tabs = elements_named(&window, "tab.");
        for (name, rect, _) in tabs.iter().filter(|(n, ..)| n != "tab.overflow" && !n.starts_with("tab.overflow.")) {
            assert!(rect.max.x <= width + 0.5, "{name} leaves a {width} point window: {rect:?}");
        }
        let shown = tabs.iter().filter(|(n, ..)| !n.starts_with("tab.overflow")).count();
        assert!(shown >= 2, "File and at least one more tab show at {width}");
        if shown < default_names().len() {
            // The rest are behind the menu button, and a click on one opens it.
            let menu = window.state().element("tab.overflow").expect("a menu button for the rest").rect;
            crate::support::click(&mut window, menu.center());
            window.step();
            window.step();
            let listed = elements_named(&window, "tab.overflow.");
            assert_eq!(listed.len() + shown, default_names().len(), "every tab is on the row or in the menu at {width}");
            assert!(window.state().element("tab.Help").is_none() || window.state().element("tab.overflow.Help").is_none());
            let (name, rect, _) = listed.last().cloned().unwrap();
            crate::support::click(&mut window, rect.center());
            window.step();
            window.step();
            assert_eq!(
                format!("tab.overflow.{}", window.state().window_state()["tab"].as_str().unwrap()),
                name,
                "the tab chosen from the menu opens"
            );
        }
    }
}

/// A window that has nothing to animate comes to rest at every width: a layout that asks for another frame forever (a scroll
/// asked for in each frame, say) would keep the app repainting and burn the processor. `drawing_window` runs until the
/// window settles, and fails if it does not.
#[test]
fn the_window_comes_to_rest_at_every_width() {
    for width in (480..=1200).step_by(20) {
        let _ = drawing_window([width as f32, 500.0]);
    }
}
