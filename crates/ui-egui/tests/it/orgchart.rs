//! In the window: the Org Chart tab shows while the drawing holds an org chart and goes when it does not, its Layout menu, Re-Layout, Arrange Subordinates and Picture group
//! send the engine's commands, and a position dropped on its manager from the Shapes window is linked. The pointer is given to egui as a person's would be; what is checked is the
//! drawing the commands changed and what the window showed.

use serde_json::{Value as Json, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, press, run_command};

fn drop_at(w: &mut Window, master: &str, x: f64, y: f64) -> Json {
    w.state_mut().execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": y})).unwrap()
}

fn id_of(answer: &Json) -> u32 {
    answer["id"].as_u64().unwrap() as u32
}

fn info(w: &mut Window, ids: &[u32]) -> Json {
    w.state_mut().execute("orgchart.info", &json!({"ids": ids})).unwrap()
}

fn tabs(w: &Window) -> Vec<String> {
    diacraft_ui_egui::ribbon::spec::tab_names(w.state().tab_context()).iter().map(|s| (*s).to_owned()).collect()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn select(w: &mut Window, ids: &[u32]) {
    run_command(w, "edit.select", json!({"ids": ids}));
}

/// A window with an executive and two managers under it (the second selected on the Org Chart tab). → the window and the three.
fn with_chart() -> (Window, u32, u32, u32) {
    let mut w = edit_window([1400.0, 900.0]);
    let boss = id_of(&drop_at(&mut w, "orgchart.executive", 7.0, 8.0));
    let m1 = id_of(&drop_at(&mut w, "orgchart.manager", 7.0, 8.0));
    let m2 = id_of(&drop_at(&mut w, "orgchart.manager", 7.0, 8.0));
    w.rest();
    (w, boss, m1, m2)
}

#[test]
fn the_org_chart_tab_shows_with_a_position_and_goes_when_the_chart_is_gone() {
    // Given an org chart in the document, Org Chart is after View and before Help.
    let mut w = edit_window([1400.0, 900.0]);
    drop_at(&mut w, "rectangle", 3.0, 4.0);
    w.rest();
    assert!(!tabs(&w).iter().any(|t| t == "Org Chart"), "a plain shape is no chart: {:?}", tabs(&w));
    drop_at(&mut w, "orgchart.manager", 6.0, 4.0);
    w.rest();
    let names = tabs(&w);
    assert_eq!(names[names.len() - 2..], ["Org Chart", "Help"], "{names:?}");
    assert_eq!(w.state().window_state()["orgChart"]["present"], true);
    // The tab is open, and one undo takes the chart away: the ribbon goes back to the first tab.
    run_command(&mut w, "window.tab", json!({"tab": "Org Chart"}));
    run_command(&mut w, "edit.undo", json!({}));
    w.rest();
    assert!(!tabs(&w).iter().any(|t| t == "Org Chart"));
    assert_eq!(w.state().window_state()["tab"], "Home", "the tab that went hands the ribbon back to the first tab");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_layout_menu_lays_the_whole_chart_out_and_marks_the_one_the_page_holds() {
    let (mut w, boss, m1, m2) = with_chart();
    run_command(&mut w, "window.tab", json!({"tab": "Org Chart"}));
    for control in [
        "OrgChartLayoutGallery",
        "OrgChartRelayout",
        "OrgChartArrangeSubordinates",
        "OrgChartPictureInsert",
        "OrgChartPictureChange",
        "OrgChartPictureDelete",
        "OrgChartPictureHide",
    ] {
        assert!(enabled(&w, &format!("ribbon.Org Chart.{control}")), "{control} is live on a chart");
    }
    for dead in ["OrgChartFitToPage", "OrgChartDataImport"] {
        assert!(!enabled(&w, &format!("ribbon.Org Chart.{dead}")), "{dead} is not built");
    }
    press(&mut w, "ribbon.Org Chart.OrgChartLayoutGallery");
    w.rest();
    press(&mut w, "menu.OrgChartLayoutLeftToRight");
    w.rest();
    let layout = w.state_mut().execute("page.layout", &json!({})).unwrap();
    assert_eq!(layout["place_style"], 23, "{layout}");
    let (b, a) = {
        let doc = w.state().session().document().unwrap();
        let at = |id: u32| {
            let c = diacraft_engine::draw::shape_corners(doc, 1, id).unwrap();
            (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
        };
        (at(boss), (at(m1), at(m2)))
    };
    assert!(b.0 < (a.0).0 && b.0 < (a.1).0, "a chart that flows to the right has its executive at the left: {b:?} {a:?}");
    assert_eq!(w.state().window_state()["orgChart"]["layout"], "left_to_right");
    press(&mut w, "ribbon.Org Chart.OrgChartLayoutGallery");
    w.rest();
    press(&mut w, "menu.OrgChartLayoutTopToBottom");
    w.rest();
    assert_eq!(w.state_mut().execute("page.layout", &json!({})).unwrap()["place_style"], 17);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn re_layout_puts_a_report_that_was_pulled_out_of_line_back() {
    let (mut w, _boss, m1, m2) = with_chart();
    let before = info(&mut w, &[m1, m2]);
    run_command(&mut w, "shape.move", json!({"ids": [m2], "dx": 3.0, "dy": -2.0}));
    run_command(&mut w, "window.tab", json!({"tab": "Org Chart"}));
    press(&mut w, "ribbon.Org Chart.OrgChartRelayout");
    w.rest();
    let doc = w.state().session().document().unwrap();
    let c = diacraft_engine::draw::shape_corners(doc, 1, m2).unwrap();
    let c1 = diacraft_engine::draw::shape_corners(doc, 1, m1).unwrap();
    let y = |c: &[(f64, f64); 4]| c.iter().map(|p| p.1).sum::<f64>() / 4.0;
    assert!((y(&c) - y(&c1)).abs() < 1.0e-6, "the two managers are in one row again");
    assert_eq!(before["links"], info(&mut w, &[])["links"]);
}

#[test]
fn arrange_subordinates_hangs_the_reports_of_the_position_selected_and_marks_its_choice() {
    let (mut w, _boss, m1, _m2) = with_chart();
    let kids: Vec<u32> = (0..2).map(|_| id_of(&drop_at(&mut w, "orgchart.position", 7.0, 8.0))).collect();
    // The two were dropped where the executive is: select the first manager and arrange its reports (there are none yet): link them first.
    for k in &kids {
        run_command(&mut w, "orgchart.link", json!({"id": k, "manager": m1}));
    }
    select(&mut w, &[m1]);
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "Org Chart"}));
    press(&mut w, "ribbon.Org Chart.OrgChartArrangeSubordinates");
    w.rest();
    press(&mut w, "menu.OrgChartArrangeVerticalRight");
    w.rest();
    let doc = w.state().session().document().unwrap();
    let at = |id: u32| {
        let c = diacraft_engine::draw::shape_corners(doc, 1, id).unwrap();
        (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
    };
    assert!((at(kids[0]).0 - at(kids[1]).0).abs() < 1.0e-6 && at(kids[0]).1 > at(kids[1]).1, "a column: {:?} {:?}", at(kids[0]), at(kids[1]));
    assert_eq!(w.state().window_state()["orgChart"]["selected"][0]["arrangement"], "vertical_right");
    // Nothing selected: refused in words.
    select(&mut w, &[]);
    w.rest();
    let e = w.state_mut().execute("window.orgChartDo", &json!({"op": "arrange", "value": "horizontal"})).unwrap_err();
    assert!(e.to_string().contains("select a position"), "{e}");
    let e = w.state_mut().execute("window.orgChartDo", &json!({"op": "wiggle"})).unwrap_err();
    assert!(e.to_string().contains("no operation `wiggle`"), "{e}");
    let e = w.state_mut().execute("window.orgChartDo", &json!({})).unwrap_err();
    assert!(e.to_string().contains("missing `op`"), "{e}");
}

#[test]
fn the_picture_group_puts_a_picture_on_the_position_selected_hides_it_and_deletes_it() {
    let (mut w, _boss, m1, _m2) = with_chart();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 12, 12);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[150u8; 12 * 12 * 4]).unwrap();
    let dir = std::env::temp_dir().join(format!("diacraft-orgchart-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("face.png");
    std::fs::write(&file, &out).unwrap();
    select(&mut w, &[m1]);
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "Org Chart"}));
    run_command(&mut w, "window.orgChartDo", json!({"op": "picture", "path": file.to_str().unwrap()}));
    w.rest();
    assert_eq!(w.state().window_state()["orgChart"]["selected"][0]["picture"], json!({"has": true, "hidden": false}));
    press(&mut w, "ribbon.Org Chart.OrgChartPictureHide");
    w.rest();
    assert_eq!(w.state().window_state()["orgChart"]["selected"][0]["picture"], json!({"has": true, "hidden": true}));
    press(&mut w, "ribbon.Org Chart.OrgChartPictureHide");
    w.rest();
    assert_eq!(w.state().window_state()["orgChart"]["selected"][0]["picture"]["hidden"], false, "pressed again it shows the picture");
    press(&mut w, "ribbon.Org Chart.OrgChartPictureDelete");
    w.rest();
    assert_eq!(w.state().window_state()["orgChart"]["selected"][0]["picture"], json!({"has": false, "hidden": false}));
    // A position needs to be selected, and a bad file is refused in words.
    select(&mut w, &[]);
    w.rest();
    let e = w.state_mut().execute("window.orgChartDo", &json!({"op": "picture", "path": file.to_str().unwrap()})).unwrap_err();
    assert!(e.to_string().contains("select a position"), "{e}");
    select(&mut w, &[m1]);
    w.rest();
    let e = w.state_mut().execute("window.orgChartDo", &json!({"op": "picture", "path": dir.join("missing.png").to_str().unwrap()})).unwrap_err();
    assert!(e.to_string().contains("no such file"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_position_dropped_from_the_shapes_window_onto_its_manager_is_linked() {
    // The Shapes window's drop is `shape.drop` at the pointer's place: the manager's box takes the drop and the chart reflows.
    let mut w = edit_window([1400.0, 900.0]);
    let boss = id_of(&drop_at(&mut w, "orgchart.executive", 7.0, 8.0));
    let r = drop_at(&mut w, "orgchart.manager", 7.0, 8.0);
    assert_eq!(r["reports_to"].as_u64(), Some(u64::from(boss)), "{r}");
    w.rest();
    assert_eq!(info(&mut w, &[])["links"], 1);
}
