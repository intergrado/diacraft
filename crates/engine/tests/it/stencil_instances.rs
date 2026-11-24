//! The instance is made from the drawing's master, a copy keeps no text of the stencil's ids, the open stencils are charged by
//! what their models hold, a master of several shapes drops as a group, and a reference to a part is found by its namespace.

use std::collections::HashSet;

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_engine::draw::shape_corners;
use diacraft_engine::stencil::{MAX_OPEN_ITEMS, MAX_OPEN_MEDIA_BYTES, MAX_OPEN_XML_BYTES, ModelCost, OpenedStencil, OpenedStencils};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::stencil_open::refusal;
use crate::stencil_open::{
    CT_STENCIL, Dir, LOOK, box_cells, decision_master, drawing_with, group_master, guid, ids, line_master, num, open, process_master, simple_master,
    stencil_bytes,
};
use crate::{blank_session, check_session, run};

fn cluster_stencil() -> Vec<u8> {
    VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&crate::stencil_open::cluster_members()).attrs(&ids(301, 401))).build()
}

// ---------------------------------------------------------------------------------------------- 1: read from the drawing's master

#[test]
fn the_instance_follows_the_drawings_master_when_matching_reuses_one_that_differs_in_pin_and_members() {
    let dir = Dir::new();
    // The drawing's master has the stencil's Process BaseID and another UniqueID, so it is found by the BaseID and used as it is. It is a group of
    // two boxes (20 and 21) 3 by 3 whose pin is its lower-left corner; the stencil's is a box 1.5 by 0.8 with its pin in the middle.
    let child = |id: u32| shape(id, &format!("{LOOK}{}", box_cells(1.0, 1.0)));
    let frame = r#"<Cell N="PinX" V="0"/><Cell N="PinY" V="0"/><Cell N="Width" V="3"/><Cell N="Height" V="3"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>"#;
    let theirs = MasterSpec::new(4, "Process").attrs(&ids(555, 201)).shapes(&shape_with(
        5,
        r#" Type="Group""#,
        &format!("{frame}<Cell N=\"FillPattern\" V=\"0\"/><Shapes>{}{}</Shapes>", child(20), child(21)),
    ));
    let mut s = drawing_with(&dir, vec![theirs]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 5.0, "y": 5.0}));
    assert_eq!((r["master"].as_u64(), r["master_added"].as_bool()), (Some(4), Some(false)), "the drawing's master is the one");
    let d = s.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    assert_eq!(inst.kind.text(), Some("Group"), "its Type is the drawing's master's");
    // Members that name the drawing's master shapes, and none of the stencil's.
    assert_eq!(inst.children.iter().map(|m| m.master_shape).collect::<Vec<_>>(), [Some(20), Some(21)]);
    // The centre of its 3 by 3 box is at (5, 5), so the pin (the lower-left corner) is at (3.5, 3.5): not where the stencil's pin would have put it.
    assert_eq!((num(d, 1, inst.id, "PinX"), num(d, 1, inst.id, "PinY")), (3.5, 3.5));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(3.0), Some(3.0)));
    let corners = shape_corners(d, 1, inst.id).unwrap();
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert!((middle.0 - 5.0).abs() < 1e-9 && (middle.1 - 5.0).abs() < 1e-9, "{middle:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_stencil_group_dropped_where_the_drawing_has_a_plain_box_lists_no_members() {
    let dir = Dir::new();
    let theirs = simple_master(4, "Cluster", &ids(555, 401));
    let mut s = drawing_with(&dir, vec![theirs]);
    open(&mut s, &dir.write("cluster.vssx", &cluster_stencil()));
    let r = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    assert_eq!((r["master"].as_u64(), r["master_added"].as_bool()), (Some(4), Some(false)));
    let d = s.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    // No member names a master shape the drawing's master lacks, and the instance is a shape, as its master's is.
    // (The drawing's master shape has no `Type`, so neither has the instance: it has its master's, R8.)
    assert_eq!((inst.children.len(), inst.kind.text()), (0, None));
    assert_eq!((num(d, 1, inst.id, "Width"), num(d, 1, inst.id, "Height")), (3.0, 3.0));
    assert_eq!((num(d, 1, inst.id, "PinX"), num(d, 1, inst.id, "PinY")), (6.0, 4.0));
}

#[test]
fn a_stencil_line_dropped_where_the_drawing_has_a_box_gets_a_box_pin_and_a_stencil_box_where_it_has_a_line_gets_ends() {
    let dir = Dir::new();
    // The stencil's Link is a line; the drawing's master of the same BaseID is a box 2 by 1 with its pin in the middle.
    let mut s = drawing_with(&dir, vec![simple_master(4, "Link", &ids(556, 203)).shapes(&shape(5, &box_cells(2.0, 1.0)))]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Link", "stencil": "file-1", "page": 1, "x": 5.0, "y": 3.0}));
    assert_eq!(r["master_added"], false);
    let d = s.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    let held: Vec<&str> = inst.sheet.cells.keys().map(|k| k.as_str()).collect();
    assert_eq!(held, ["PinX", "PinY"], "a box: its pin, not the ends of a line");
    assert_eq!((num(d, 1, inst.id, "PinX"), num(d, 1, inst.id, "PinY")), (5.0, 3.0));
    // The other way: the stencil's Decision is a box; the drawing's master of its BaseID is a line from (0, 0) to (2, 0).
    let line = line_master().attrs(&ids(557, 202));
    let line = MasterSpec { id: "4".to_owned(), name: "Decision".to_owned(), ..line };
    let mut t = drawing_with(&dir, vec![line]);
    open(&mut t, &dir.write("shapes2.vssx", &stencil_bytes()));
    let r = run(&mut t, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 5.0, "y": 3.0}));
    assert_eq!(r["master_added"], false);
    let d = t.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    let local: Vec<&str> =
        inst.sheet.cells.iter().filter(|(_, c)| !matches!(c.formula, diacraft_doc::FormulaSlot::Inherit)).map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        local,
        ["BeginX", "BeginY", "EndX", "EndY"],
        "what the instance holds of its own: the ends; its pin is its master's formula, worked out"
    );
    assert_eq!((num(d, 1, inst.id, "BeginX"), num(d, 1, inst.id, "EndX"), num(d, 1, inst.id, "PinX")), (4.0, 6.0, 5.0));
}

// ---------------------------------------------------------------------------------------- 2: ids and links in the saved file

/// The attributes of each `<tag ...>` start tag of `xml`, as `(name, value)` pairs.
fn start_tags(xml: &str, tag: &str) -> Vec<Vec<(String, String)>> {
    let open = format!("<{tag}");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        rest = rest.get(at + open.len()..).unwrap_or("");
        if !rest.starts_with([' ', '/', '>']) {
            continue;
        }
        let end = rest.find('>').unwrap_or(rest.len());
        let mut inside = rest.get(..end).unwrap_or("");
        let mut attrs = Vec::new();
        while let Some(eq) = inside.find("=\"") {
            let name = inside.get(..eq).unwrap_or("").trim().to_owned();
            let after = inside.get(eq + 2..).unwrap_or("");
            let close = after.find('"').unwrap_or(after.len());
            attrs.push((name, after.get(..close).unwrap_or("").to_owned()));
            inside = after.get(close + 1..).unwrap_or("");
        }
        out.push(attrs);
    }
    out
}

fn attr<'a>(tag: &'a [(String, String)], name: &str) -> Option<&'a str> {
    tag.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn canonical(id: &str) -> bool {
    id == "0" || (!id.starts_with('0') && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn a_copied_master_and_style_are_saved_under_the_ids_the_drawing_gave_them_and_their_links_stay() {
    let dir = Dir::new();
    // The stencil writes its ids and links with leading zeros (`ID="05"`, `LineStyle="03"`), which the model keeps as text and a write gives back, and a
    // master shape that names masters (`Master="9"`), which a master part never has.
    let styles = r#"<StyleSheet ID="04" NameU="Flow Base" Name="Flow Base" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.0138889"/></StyleSheet><StyleSheet ID="03" NameU="Flow Normal" Name="Flow Normal" LineStyle="04" FillStyle="04" TextStyle="04"><Cell N="FillPattern" V="1"/></StyleSheet>"#;
    let padded = format!(r#"<Shape ID="05" LineStyle="03" FillStyle="03" TextStyle="03" Master="9" MasterShape="9">{}</Shape>"#, box_cells(1.0, 1.0));
    let master = MasterSpec { id: "05".to_owned(), ..MasterSpec::new(5, "Padded") }.attrs(&ids(701, 801)).shapes(&padded);
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).more_styles(styles).master(master).build();
    // The drawing has masters 5 and 9, and a style 3 that is another style, so the copy's id and its links must change.
    let target = VsdxBuilder::new()
        .more_styles(
            r#"<StyleSheet ID="3" NameU="Mine" Name="Mine" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.05"/></StyleSheet>"#,
        )
        .master(simple_master(5, "Five", &ids(1, 2)))
        .master(simple_master(9, "Nine", &ids(3, 4)))
        .page(diacraft_testkit::package::PageSpec::new(1, "Page-1"))
        .build();
    let mut s = blank_session();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("padded.vssx", &stencil));
    let r = run(&mut s, "shape.drop", json!({"master": "Padded", "stencil": "file-1", "page": 1, "x": 3.0, "y": 3.0}));
    assert_eq!(r["master"], 10, "one above the drawing's highest");
    let out = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": out}));

    let package = Package::read(&std::fs::read(&out).unwrap(), &Limits::default()).unwrap();
    let text = |suffix: &str| -> String {
        let name = package.parts().find(|p| p.to_ascii_lowercase().ends_with(suffix)).unwrap().to_owned();
        String::from_utf8(package.part(&name).unwrap().to_vec()).unwrap()
    };
    // Every master id and every style id in the saved file is a plain number and is on one master or one style only.
    let masters = start_tags(&text("masters/masters.xml"), "Master");
    let master_ids: Vec<&str> = masters.iter().filter_map(|t| attr(t, "ID")).collect();
    assert_eq!(master_ids.len(), 3);
    assert!(master_ids.iter().all(|id| canonical(id)), "{master_ids:?}");
    assert_eq!(master_ids.iter().collect::<HashSet<_>>().len(), 3, "no id twice: {master_ids:?}");
    let document = text("visio/document.xml");
    let styles = start_tags(&document, "StyleSheet");
    let style_ids: Vec<&str> = styles.iter().filter_map(|t| attr(t, "ID")).collect();
    assert!(style_ids.iter().all(|id| canonical(id)), "{style_ids:?}");
    assert_eq!(style_ids.iter().collect::<HashSet<_>>().len(), style_ids.len(), "no id twice: {style_ids:?}");
    let named = |name: &str| styles.iter().find(|t| attr(t, "NameU") == Some(name)).unwrap_or_else(|| panic!("{name}"));
    let (normal, base) = (attr(named("Flow Normal"), "ID").unwrap(), attr(named("Flow Base"), "ID").unwrap());
    assert!(normal != "3" && base != "3" && normal != base, "the drawing's own style 3 is not taken: {normal} {base}");
    assert_eq!(attr(named("Mine"), "ID"), Some("3"));
    // The copy of 3 is still based on the copy of 4 after the save: its links are what the copy was given, not the text of the stencil's.
    let flow = named("Flow Normal");
    let number = |v: Option<&str>| v.and_then(|t| t.parse::<u32>().ok());
    let want = base.parse::<u32>().ok();
    assert_eq!([number(attr(flow, "LineStyle")), number(attr(flow, "FillStyle")), number(attr(flow, "TextStyle"))], [want; 3]);
    // The copied master's shape is linked to the copy of 3, and names no master.
    let mut seen = 0;
    for name in package
        .parts()
        .filter(|p| p.to_ascii_lowercase().contains("masters/master") && !p.ends_with("masters.xml"))
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let xml = String::from_utf8(package.part(&name).unwrap().to_vec()).unwrap();
        for tag in start_tags(&xml, "Shape") {
            assert!(attr(&tag, "Master").is_none() && attr(&tag, "MasterShape").is_none(), "{name}: {tag:?}");
            if let Some(line) = attr(&tag, "LineStyle") {
                seen += 1;
                let got = [line, attr(&tag, "FillStyle").unwrap(), attr(&tag, "TextStyle").unwrap()].map(|t| t.parse::<u32>().ok());
                assert_eq!(got, [normal.parse::<u32>().ok(); 3], "{name}: the copy's links, not the stencil's `03`");
            }
        }
    }
    assert_eq!(seen, 1, "the one shape that has links");
    // Read back, the model says the same.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": out}));
    let e = t.document().unwrap();
    let copy = e.masters.iter().find(|m| m.name.as_deref() == Some("Padded")).unwrap();
    let links = copy.shapes.get(0).unwrap().sheet.links;
    let flow_id: u32 = normal.parse().unwrap();
    assert_eq!((links.line, links.fill, links.text), (Some(flow_id), Some(flow_id), Some(flow_id)));
    assert_eq!(e.style(3).unwrap().name_u.as_deref(), Some("Mine"));
    assert_eq!(check_session(&t), Ok(()));
}

// ------------------------------------------------------------------------------------------------- 3: memory of the open stencils

#[test]
fn what_the_open_stencils_hold_is_summed_over_the_readers_limits_for_one_file() {
    let cost = |xml_mib: u64, items: u64, media_mib: u64| ModelCost {
        xml_bytes: xml_mib * 1024 * 1024,
        items,
        media_bytes: media_mib * 1024 * 1024,
        memory_bytes: 0,
    };
    let mut list = OpenedStencils::new();
    let (items, media_mib) = (MAX_OPEN_ITEMS, MAX_OPEN_MEDIA_BYTES / (1024 * 1024));
    assert_eq!((items, media_mib), (12_000_000, 512), "the reader's limits for one file (`diacraft_vsdx::MAX_ITEMS` and `MAX_MEDIA_BYTES`)");
    assert_eq!(list.fits(&cost(512, items, media_mib)), Ok(()), "one file at the reader's limits fits alone");
    let id = list.next_id();
    list.add(OpenedStencil::new(id, "A".to_owned(), None, 1_000, cost(300, 5_000_000, 100), Document::empty()));
    assert_eq!(list.fits(&cost(212, items - 5_000_000, media_mib - 100)), Ok(()), "to the byte, on each of the three");
    assert!(list.fits(&cost(213, 0, 0)).is_err(), "XML");
    assert!(list.fits(&cost(0, items - 5_000_000 + 1, 0)).is_err(), "items");
    assert!(list.fits(&cost(0, 0, media_mib - 100 + 1)).is_err(), "pictures");
    assert_eq!(MAX_OPEN_XML_BYTES, 512 * 1024 * 1024);
}

#[test]
fn a_file_of_a_few_hundred_kilobytes_that_inflates_is_charged_for_what_it_holds() {
    let dir = Dir::new();
    // Four master parts of 24 MiB of XML each (a comment of a block repeated, which deflates twenty to one): a file of under five megabytes that
    // takes 96 MiB of XML, so five of them are 480 MiB and the sixth is past the 512 MiB one file may take. By the old measure, the size of the
    // file, all sixteen of them (75 MB of files, 1.5 GiB of XML) would have been taken.
    let block: String = (0..1024).map(|i| char::from(b'a' + u8::try_from((i * 7 + i / 26) % 26).unwrap())).collect();
    let comment = format!("<!--{}-->", block.repeat(24 * 1024));
    let part = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><MasterContents xmlns="{NS_2012}" xmlns:r="{NS_R}">{comment}<Shapes>{}</Shapes></MasterContents>"#,
        shape(5, &box_cells(1.0, 1.0))
    );
    let mut b = VsdxBuilder::new().document_type(CT_STENCIL);
    for k in 1..=4_u32 {
        b = b.master(MasterSpec::new(k, &format!("Big {k}")).part(&part));
    }
    let bytes = b.build();
    println!("file: {} bytes", bytes.len());
    assert!(bytes.len() < 8_000_000, "{}", bytes.len());
    let mut s = blank_session();
    let mut opened = 0;
    let mut refused = None;
    for k in 0..16 {
        match s.execute("stencil.open", &json!({"path": dir.write(&format!("big{k}.vssx"), &bytes)})) {
            Ok(_) => opened += 1,
            Err(e) => {
                refused = Some(e.to_string());
                break;
            }
        }
    }
    assert_eq!(opened, 5);
    let msg = refused.expect("the sixth is refused");
    assert!(msg.contains("512 MiB") && msg.contains("stencil.close"), "{msg}");
    assert_eq!(run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len(), 7 + 5, "the refused one is not open");
    // Closing one makes room for it.
    run(&mut s, "stencil.close", json!({"id": "file-1"}));
    run(&mut s, "stencil.open", json!({"path": dir.path("big5.vssx")}));
}

// ----------------------------------------------------------------------------------------------- 5: a master of several top-level shapes

/// A master of two top-level shapes: A, a box 2 by 1 at pin (1, 0.5), and B, a box 2 by 1 whose pin is its lower-left corner at (5, 2), turned
/// a quarter. B's corners on the master's page are (5, 2), (5, 4), (4, 4) and (4, 2), so the joint box is from (0, 0) to (5, 4).
fn pair_stencil() -> Vec<u8> {
    let a = shape(5, &format!("{LOOK}{}", box_cells(2.0, 1.0)));
    let b = shape(
        6,
        &format!(
            "{LOOK}<Cell N=\"PinX\" V=\"5\"/><Cell N=\"PinY\" V=\"2\"/><Cell N=\"Width\" V=\"2\"/><Cell N=\"Height\" V=\"1\"/><Cell N=\"LocPinX\" V=\"0\"/><Cell N=\"LocPinY\" V=\"0\"/><Cell N=\"Angle\" V=\"1.5707963267949\"/>{}",
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
        ),
    );
    VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Pair").attrs(&ids(901, 902)).shapes(&format!("{a}{b}"))).build()
}

#[test]
fn a_master_of_several_top_level_shapes_drops_as_a_group_with_a_member_for_each_and_the_joint_box_for_its_frame() {
    let dir = Dir::new();
    let mut s = blank_session();
    let opened = open(&mut s, &dir.write("pair.vssx", &pair_stencil()));
    assert_eq!(opened["masters"][0]["name"], "Pair");
    let r = run(&mut s, "shape.drop", json!({"master": "Pair", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(5.0), Some(4.0)), "the joint box, a turned member's corners included");
    let d = s.document().unwrap();
    let group = d.page(1).unwrap().shapes.get(0).unwrap();
    assert_eq!((group.id, group.kind.text(), group.master), (1, Some("Group"), Some(1)));
    assert_eq!(group.children.iter().map(|m| (m.id, m.master_shape)).collect::<Vec<_>>(), [(2, Some(5)), (3, Some(6))]);
    // The group's frame: its box, its pin at the place, the middle of the box as its local pin.
    let cell = |name: &str| num(d, 1, 1, name);
    assert_eq!((cell("Width"), cell("Height"), cell("PinX"), cell("PinY"), cell("LocPinX"), cell("LocPinY")), (5.0, 4.0, 6.0, 4.0, 2.5, 2.0));
    // A member's pin is in the group's frame: its pin on the master's page less the corner of the box.
    assert_eq!((num(d, 1, 2, "PinX"), num(d, 1, 2, "PinY")), (1.0, 0.5));
    assert_eq!((num(d, 1, 3, "PinX"), num(d, 1, 3, "PinY")), (5.0, 2.0));
    // On the page A's box is its master's, moved so that the joint box is centred on (6, 4): from (3.5, 2) to (5.5, 3).
    let a = shape_corners(d, 1, 2).unwrap();
    assert!((a[0].0 - 3.5).abs() < 1e-9 && (a[0].1 - 2.0).abs() < 1e-9 && (a[2].0 - 5.5).abs() < 1e-9 && (a[2].1 - 3.0).abs() < 1e-9, "{a:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_of_a_master_moves_whole_is_resized_with_its_members_and_saves_and_reads_back() {
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("pair.vssx", &pair_stencil()));
    run(&mut s, "shape.drop", json!({"master": "Pair", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    // A move carries the group's pin, and the members' pins, in the group's frame, are not touched.
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": -1.0}));
    let d = s.document().unwrap();
    assert_eq!((num(d, 1, 1, "PinX"), num(d, 1, 1, "PinY")), (7.0, 3.0));
    assert_eq!((num(d, 1, 2, "PinX"), num(d, 1, 2, "PinY")), (1.0, 0.5));
    let a = shape_corners(d, 1, 2).unwrap();
    assert!((a[0].0 - 4.5).abs() < 1e-9 && (a[0].1 - 1.0).abs() < 1e-9, "{a:?}");
    // A group is sized as one shape and its members follow it: their corners go where the stretch of the group's box puts them.
    let (g, m2, m3) = (shape_corners(d, 1, 1).unwrap(), shape_corners(d, 1, 2).unwrap(), shape_corners(d, 1, 3).unwrap());
    let (w, h) = (g[1].0 - g[0].0, g[3].1 - g[0].1);
    run(&mut s, "shape.resize", json!({"id": 1, "x": g[0].0, "y": g[0].1, "width": 2.0 * w, "height": 1.5 * h}));
    let d = s.document().unwrap();
    for (id, was) in [(2, m2), (3, m3)] {
        for (p, q) in was.iter().zip(shape_corners(d, 1, id).unwrap().iter()) {
            let want = (g[0].0 + (p.0 - g[0].0) * 2.0, g[0].1 + (p.1 - g[0].1) * 1.5);
            assert!((q.0 - want.0).abs() < 1e-9 && (q.1 - want.1).abs() < 1e-9, "member {id}: {q:?} for {want:?}");
        }
    }
    // Turned, it turns as a group, its members with it.
    run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 90}));
    let path = dir.path("pair.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let (d, e) = (s.document().unwrap(), t.document().unwrap());
    let group = e.page(1).unwrap().shapes.get(0).unwrap();
    assert_eq!(group.children.iter().map(|m| (m.id, m.master_shape)).collect::<Vec<_>>(), [(2, Some(5)), (3, Some(6))]);
    for cell in ["Width", "Height", "PinX", "PinY", "LocPinX", "LocPinY", "Angle"] {
        assert!((num(d, 1, 1, cell) - num(e, 1, 1, cell)).abs() < 1e-9, "{cell}");
    }
    for id in [2, 3] {
        let (x, y) = (shape_corners(d, 1, id).unwrap(), shape_corners(e, 1, id).unwrap());
        assert!(x.iter().zip(y.iter()).all(|(p, q)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9), "{id}: {x:?} {y:?}");
    }
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn a_group_of_several_shapes_is_drawn_with_its_members_in_place() {
    use diacraft_render::{Renderer, View};
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("pair.vssx", &pair_stencil()));
    run(&mut s, "shape.drop", json!({"master": "Pair", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, 100.0).unwrap();
    let r = Renderer::new().render(doc, 1, &view);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let px = |x: f64, y: f64| {
        let (w, h) = (r.width as usize, r.height as usize);
        let col = ((x * 100.0) as usize).min(w - 1);
        let row = (((8.5 - y) * 100.0) as usize).min(h - 1);
        let at = (row * w + col) * 4;
        [r.rgba_premul[at], r.rgba_premul[at + 1], r.rgba_premul[at + 2], r.rgba_premul[at + 3]]
    };
    // The joint box (0..5, 0..4 on the master's page) is centred on (6, 4), so it spans x 3.5 to 8.5 and y 2 to 6 on the page. A is the box from (3.5, 2)
    // to (5.5, 3); B, turned, covers x 4 to 5 and y 2 to 4 on the master's page, so x 7.5 to 8.5 and y 4 to 6 on the page.
    assert_eq!(px(4.5, 2.5), [0xE8, 0xF0, 0xFF, 255], "A");
    assert_eq!(px(8.0, 5.0), [0xE8, 0xF0, 0xFF, 255], "B, the turned one");
    assert_eq!(px(4.5, 5.5), [255, 255, 255, 255], "nothing in the corner of the box that holds no member");
}

// ------------------------------------------------------------------------------------------ 6: a reference to a part, by its namespace

fn part_with(shapes: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><MasterContents xmlns="{NS_2012}" xmlns:r="{NS_R}" xmlns:rel="{NS_R}"><Shapes>{shapes}</Shapes></MasterContents>"#
    )
}

#[test]
fn a_relationship_attribute_is_found_by_its_namespace_whatever_its_prefix() {
    let dir = Dir::new();
    let with = |extra: &str| MasterSpec::new(1, "M").part(&part_with(&shape(5, &format!("{}{extra}", box_cells(1.0, 1.0)))));
    let cases: [(&str, bool); 6] = [
        // Bound to the namespace by the part's root under another prefix.
        (r#"<Mystery rel:id="rId3"/>"#, true),
        // Bound to it by the element itself under a prefix of its own.
        (&format!(r#"<Mystery xmlns:q="{NS_R}" q:embed="rId4"/>"#), true),
        // Bound by an element above, inside the kept text.
        (&format!(r#"<Outer xmlns:q="{NS_R}"><Inner><Deep q:link="rId5"/></Inner></Outer>"#), true),
        // `r` bound to another namespace by the element: not a relationship.
        (r#"<Mystery xmlns:r="urn:other" r:id="7"/>"#, false),
        // Another namespace's `id`, with the prefix bound above and out of sight: judged by its name, and refused.
        (r#"<Mystery foo:id="7"/>"#, true),
        // No namespace: not a relationship.
        (r#"<Mystery id="7" embed="x"/>"#, false),
    ];
    for (k, (extra, refused)) in cases.into_iter().enumerate() {
        let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(with(extra)).build();
        let mut s = blank_session();
        open(&mut s, &dir.write(&format!("m{k}.vssx"), &bytes));
        let drop = s.execute("shape.drop", &json!({"master": "M", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
        match (refused, drop) {
            (true, Err(e)) => assert!(e.to_string().contains("of the stencil's file"), "{extra}: {e}"),
            (false, Ok(_)) => {}
            (expected, other) => panic!("{extra}: refused {expected}, got {other:?}"),
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_relationship_kept_on_the_masters_own_row_is_refused_too() {
    let dir = Dir::new();
    let row = MasterSpec::new(1, "M").attrs(&format!(r#" xmlns:q="{NS_R}" q:id="rId9""#)).shapes(&shape(5, &box_cells(1.0, 1.0)));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(row).master(decision_master()).master(process_master()).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("row.vssx", &bytes));
    let msg = refusal(&mut s, "shape.drop", json!({"master": "M", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert!(msg.contains("of the stencil's file"), "{msg}");
    // The other masters of the file drop, and the refusal left nothing behind.
    run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
}

// ------------------------------------------------------------------------------------------------------------------------------ minors

#[test]
fn a_renewed_unique_id_is_said_in_the_notes() {
    let dir = Dir::new();
    let mut s = drawing_with(&dir, vec![simple_master(4, "Unrelated", &ids(101, 999))]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let notes: Vec<&str> = r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(notes.iter().any(|n| n.contains("UniqueID") && n.contains(&guid(101))), "{notes:?}");
}

/// The nine masters of that stood for the rest, dropped one after the other into an `.odg` drawing: the drop warns of nothing and one save writes
/// all of them (the gate that refused them is gone; `odg_stencils.rs` reads each back).
#[test]
fn batch_masters_drop_without_a_warning_and_are_saved_to_an_odg_drawing_together() {
    use crate::odg_build::{Dir as OdgDir, Drawing, open_drawing};
    let dir = OdgDir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(""));
    let original = dir.read("a.odg");
    let masters = [
        "decorative.star-5",
        "decorative.star-8",
        "decorative.burst",
        "arrows.four-way",
        "arrows.right",
        "gane-sarson.interface",
        "gane-sarson.process",
        "gane-sarson.data-store",
        "gane-sarson.data-flow",
    ];
    for (n, master) in masters.iter().enumerate() {
        let dropped = run(&mut s, "shape.drop", json!({"master":master,"page":0,"x":2.0 + n as f64,"y":3}));
        assert!(dropped.get("warnings").is_none(), "{master}: {dropped}");
    }
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true);
    assert_ne!(dir.read("a.odg"), original);
    assert_eq!(s.document().unwrap().pages.get(0).unwrap().shapes.len(), masters.len());
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], false, "a second save is no edit");
}

#[test]
fn a_group_of_a_copy_of_a_batch_master_and_a_rectangle_is_saved_whatever_the_master_is_called() {
    use crate::odg_build::{Dir as OdgDir, Drawing, open_drawing};
    use std::sync::Arc;
    let dir = OdgDir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(""));
    let made = run(&mut s, "shape.drop", json!({"master":"gane-sarson.interface","page":0,"x":3,"y":3}));
    let copy = run(&mut s, "shape.duplicate", json!({"page":0,"ids":[made["id"]]}))["ids"][0].clone();
    run(&mut s, "shape.delete", json!({"page":0,"ids":[made["id"]]}));
    let other = run(&mut s, "shape.drop", json!({"master":"rectangle","page":0,"x":5,"y":3}))["id"].clone();
    run(&mut s, "shape.group", json!({"page":0,"ids":[copy,other]}));
    // A copy may be renamed and receive another UniqueID on import: the master's name is no part of what is written.
    s.edit("Renew identity", |doc| {
        let master = Arc::make_mut(doc.masters.get_mut(0).unwrap());
        master.name = Some(Arc::from("Renamed"));
        master.unique_id = Some(Arc::from("{D1AC0003-0000-4000-8000-000000009999}"));
        Ok(())
    })
    .unwrap();
    let path = dir.path("copy.odg");
    s.execute("file.save_as", &json!({"path":path})).unwrap();
    let mut again = diacraft_engine::Session::new();
    run(&mut again, "file.open", json!({"path": path}));
    let page = again.document().unwrap().pages.get(0).unwrap().clone();
    assert_eq!((page.shapes.len(), page.shapes.get(0).unwrap().children.len()), (1, 2), "the group and its two members");
    assert!(!s.is_dirty(), "saved");
}

#[test]
fn a_labeled_data_flow_is_saved_with_its_direction_and_text_and_an_unused_master_does_not_block_save() {
    use crate::odg_build::{Dir as OdgDir, Drawing, open_drawing};
    let dir = OdgDir::new();
    let mut s = open_drawing(&dir, &Drawing::new().page(""));
    let made = run(&mut s, "shape.drop", json!({"master":"gane-sarson.data-flow","page":0,"x":3,"y":3}));
    run(&mut s, "shape.set_text", json!({"page":0,"id":made["id"],"text":"Order data"}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true);
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let mut again = diacraft_engine::Session::new();
    run(&mut again, "file.open", json!({"path": dir.path("a.odg")}));
    let doc = again.document().unwrap();
    let flow = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let arrow = flow.sheet.cell("EndArrow").and_then(|c| c.value.as_ref().and_then(diacraft_doc::Value::number));
    assert_eq!(arrow, Some(4.0), "the direction: an arrowhead at the end");
    let words = diacraft_engine::draw::text_view(doc, doc.pages.get(0).unwrap().id, flow.id).map(|v| v.chars);
    assert_eq!(words.as_deref(), Some("Order data"), "the label");
    run(&mut s, "shape.delete", json!({"page":0,"ids":[made["id"]]}));
    assert_eq!(s.document().unwrap().masters.len(), 1, "deletion keeps an unused master");
    run(&mut s, "file.save", json!({}));
}
