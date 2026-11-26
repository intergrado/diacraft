//! Network Devices and Computers and Devices: the twelve and the sixteen masters, their glyphs inside their icon boxes, their labels and Shape Data, the
//! backbone's points, the Comm-link, the Quick Shapes, the search words and the template. The catalogue's invariants (`stencil/catalogue_tests.rs`) and the five per-stencil checks
//! (`tests/it/stencils2_network.rs`, `stencils2_computers.rs`) run on every master beside these.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{Document, Owner, Resolver, SectionKey, SheetRef};
use kurbo::Shape;
use serde_json::{Value, json};

use super::glyphs;
use crate::stencil::{MasterSpec, computers};
use crate::{Session, draw};

// ---- what the design lists ------------------------------------------------------------------------------------------------------------

/// The twelve masters of Network Devices as the design's table gives them: id, name, width, height (inches), and how many connection points.
const NETWORK: [(&str, &str, f64, f64, usize); 12] = [
    ("network.router", "Router", 1.0, 0.6, 4),
    ("network.switch", "Switch", 1.2, 0.5, 4),
    ("network.firewall", "Firewall", 0.9, 0.8, 4),
    ("network.cloud", "Cloud", 1.4, 0.9, 4),
    ("network.wireless-access-point", "Wireless Access Point", 0.9, 0.8, 4),
    ("network.load-balancer", "Load Balancer", 1.0, 0.6, 4),
    ("network.modem", "Modem", 1.0, 0.5, 4),
    ("network.internet", "Internet", 0.9, 0.9, 8),
    ("network.ethernet", "Ethernet", 6.0, 0.12, 18),
    ("network.ring-network", "Ring Network", 2.0, 2.0, 8),
    ("network.comm-link", "Comm-link", 1.5, 0.0, 0),
    ("network.link", "Network Link", 1.5, 0.0, 0),
];

/// The sixteen masters of Computers and Devices.
const COMPUTERS: [(&str, &str, f64, f64, usize); 16] = [
    ("computers.server", "Server", 0.6, 1.0, 4),
    ("computers.desktop", "Desktop", 1.0, 0.9, 4),
    ("computers.laptop", "Laptop", 1.1, 0.75, 4),
    ("computers.database-server", "Database Server", 0.8, 1.0, 4),
    ("computers.web-server", "Web Server", 0.8, 1.0, 4),
    ("computers.file-server", "File Server", 0.8, 1.0, 4),
    ("computers.mail-server", "Mail Server", 0.8, 1.0, 4),
    ("computers.application-server", "Application Server", 0.8, 1.0, 4),
    ("computers.print-server", "Print Server", 0.8, 1.0, 4),
    ("computers.rack-server", "Rack Server", 1.2, 0.35, 4),
    ("computers.tablet", "Tablet", 0.6, 0.8, 4),
    ("computers.mobile-phone", "Mobile Phone", 0.4, 0.75, 4),
    ("computers.telephone", "Telephone", 0.8, 0.6, 4),
    ("computers.printer", "Printer", 0.9, 0.7, 4),
    ("computers.storage", "Storage", 0.9, 0.8, 4),
    ("computers.user", "User", 0.6, 0.8, 4),
];

/// Every master of the two stencils with the stencil's id.
fn all() -> Vec<(&'static str, &'static MasterSpec)> {
    super::MASTERS.iter().map(|m| ("network", m)).chain(computers::MASTERS.iter().map(|m| ("computers", m))).collect()
}

/// The two-dimensional masters: the devices.
fn devices() -> Vec<(&'static str, &'static MasterSpec)> {
    all().into_iter().filter(|(_, m)| !m.is_connector()).collect()
}

fn master(id: &str) -> &'static MasterSpec {
    all().into_iter().map(|(_, m)| m).find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))
}

/// Asserts that the twenty-eight masters are there, so that a test that loops over them is not a loop over nothing.
fn all_present() {
    assert_eq!((super::MASTERS.len(), computers::MASTERS.len()), (12, 16), "the stencils hold their twelve and sixteen masters");
}

/// A page with the master dropped at (5, 5): the document, the page and the shape's `ID`.
fn dropped(spec: &MasterSpec) -> (Document, u32, u32) {
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, spec, 5.0, 5.0).unwrap();
    (doc, page, info.id)
}

/// A session with a new drawing.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// Drops a built-in master of a session's drawing at (x, y) and answers its `ID`.
fn put(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": id, "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

// ---- the catalogue of the design ---------------------------------------------------------------------------------------------------------

#[test]
fn network_devices_holds_the_twelve_masters_of_the_design_in_order_with_their_sizes_and_points() {
    assert_eq!(super::MASTERS.len(), 12);
    for (at, (m, (id, name, w, h, points))) in super::MASTERS.iter().zip(NETWORK).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (17, u16::try_from(at + 1).unwrap()), "{id}: series 17, in the window's order");
    }
}

#[test]
fn computers_and_devices_holds_the_sixteen_masters_of_the_design_in_order_with_their_sizes_and_points() {
    assert_eq!(computers::MASTERS.len(), 16);
    for (at, (m, (id, name, w, h, points))) in computers::MASTERS.iter().zip(COMPUTERS).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (18, u16::try_from(at + 1).unwrap()), "{id}: series 18, in the window's order");
    }
}

#[test]
fn both_stencils_ship_and_stay_closed_in_a_new_session() {
    let (network, computers) = (crate::stencil::builtin_stencil("network"), crate::stencil::builtin_stencil("computers"));
    assert_eq!(network.map(|s| (s.name, s.category, s.default_open, s.masters().len())), Some(("Network Devices", "Network", false, 12)));
    assert_eq!(computers.map(|s| (s.name, s.category, s.default_open, s.masters().len())), Some(("Computers and Devices", "Network", false, 16)));
    let listed = crate::stencil::list();
    let open: Vec<&str> = listed["stencils"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!open.contains(&"network") && !open.contains(&"computers"), "a new session opens neither: {open:?}");
    let available: Vec<&str> = listed["available"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(available.contains(&"network") && available.contains(&"computers"), "More Shapes lists both: {available:?}");
}

#[test]
fn the_two_lines_are_a_fixed_zigzag_that_glues_and_a_right_angle_connector_with_no_ends() {
    let zigzag = master("network.comm-link");
    assert!(zigzag.is_connector() && zigzag.is_fixed_1d(), "the Comm-link draws its own zigzag and is not routed");
    assert_eq!(zigzag.look().map(|l| l.numbers()), Some((0, 0, 1)), "no ends, solid");
    let link = master("network.link");
    assert!(link.is_connector() && !link.is_fixed_1d(), "the Network Link is an ordinary connector");
    assert_eq!(link.look().map(|l| (l.numbers(), l.route)), Some(((0, 0, 1), super::super::Route::RightAngle)), "no ends, solid, right-angle route");
    // A drop of the link has the route style 1 (right angle) and the zigzag keeps its marker-less drop.
    let (doc, page, id) = dropped(link);
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
    let route =
        Resolver::new(&doc).resolve(&sheet, &diacraft_doc::CellAddr::direct("ShapeRouteStyle")).and_then(|f| f.value.cell?.value.as_ref()?.number());
    assert_eq!(route, Some(1.0));
    for line in [zigzag, link] {
        assert!(line.dress().is_none(), "{}: a line holds no device data", line.id);
    }
    // The listing gives both their ends and pattern (a window draws a tile from them).
    let mut s = session();
    run(&mut s, "stencil.open", json!({ "id": "network" }));
    let listed = run(&mut s, "stencil.list", json!({}));
    let network = listed["stencils"].as_array().unwrap().iter().find(|t| t["id"] == "network").expect("the stencil is open now");
    for id in ["network.comm-link", "network.link"] {
        let entry = network["masters"].as_array().unwrap().iter().find(|m| m["id"] == id).unwrap();
        assert_eq!((entry["one_d"].clone(), entry["ends"].clone()), (json!(true), json!({ "begin": 0, "end": 0, "pattern": 1 })), "{id}");
    }
}

#[test]
fn every_two_dimensional_master_holds_objtype_1_and_the_two_lines_hold_none() {
    all_present();
    for (_, m) in devices() {
        let (doc, page, id) = dropped(m);
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
        let found =
            Resolver::new(&doc).resolve(&sheet, &diacraft_doc::CellAddr::direct("ObjType")).and_then(|f| f.value.cell?.value.as_ref()?.number());
        assert_eq!(found, Some(1.0), "{}: a shape people join", m.id);
    }
}

// ---- the glyphs ----------------------------------------------------------------------------------------------------------------------

/// The shape's box on the page, as (left, bottom, right, top).
fn box_of(doc: &Document, page: u32, id: u32) -> (f64, f64, f64, f64) {
    let f = draw::shape_frame(doc, page, id).unwrap();
    (f.pin_x - f.width / 2.0, f.pin_y - f.height / 2.0, f.pin_x + f.width / 2.0, f.pin_y + f.height / 2.0)
}

/// The box that everything the shape draws lies in, as (left, bottom, right, top), or `None` when it draws nothing.
fn ink_box(doc: &Document, page: u32, id: u32) -> Option<(f64, f64, f64, f64)> {
    let reader = draw::Reader::new(doc);
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    let outline = crate::snap::outline_for(&reader, &at, 1.0, &mut crate::snap::Budget::new(), crate::snap::Purpose::Box);
    outline.paths.iter().map(|p| p.bounding_box()).reduce(|a, b| a.union(b)).map(|r| (r.x0, r.y0, r.x1, r.y1))
}

#[test]
fn every_glyphs_strokes_lie_inside_its_icon_box() {
    assert_eq!(devices().len(), 26, "ten devices of the network and sixteen computers");
    for (_, m) in devices() {
        // At the default size, and stretched to twice the width and half the height: a glyph is shares of its box, so it stays in it.
        for (stretch_w, stretch_h) in [(1.0, 1.0), (2.0, 0.5)] {
            let (mut doc, page, id) = dropped(m);
            if stretch_w != 1.0 {
                let f = draw::shape_frame(&doc, page, id).unwrap();
                let (w, h) = (f.width * stretch_w, f.height * stretch_h);
                draw::resize_shape(&mut doc, page, id, f.pin_x - w / 2.0, f.pin_y - h / 2.0, w, h).unwrap();
            }
            let (left, bottom, right, top) = box_of(&doc, page, id);
            let ink = ink_box(&doc, page, id).unwrap_or_else(|| panic!("{}: it draws nothing", m.id));
            let slack = 1e-6;
            assert!(
                ink.0 >= left - slack && ink.1 >= bottom - slack && ink.2 <= right + slack && ink.3 <= top + slack,
                "{}: strokes {ink:?} lie outside the icon box {:?} at {stretch_w} by {stretch_h}",
                m.id,
                (left, bottom, right, top)
            );
            // And it is a pictogram of its box, not a speck in a corner: it spans nearly all of both sides.
            assert!(
                ink.2 - ink.0 >= 0.9 * (right - left) && ink.3 - ink.1 >= 0.9 * (top - bottom),
                "{}: the glyph spans {:?} of the box {:?}",
                m.id,
                ink,
                (left, bottom, right, top)
            );
        }
    }
}

#[test]
fn the_comm_links_zigzag_is_five_legs_a_fifth_of_an_inch_across_the_line() {
    let (doc, page, id) = dropped(master("network.comm-link"));
    let rows = zigzag_rows(&doc, page, id);
    assert_eq!(rows.len(), 6, "a start and five legs");
    let width = 1.5;
    let along: Vec<f64> = rows.iter().map(|r| r.1 / width).collect();
    assert_eq!(along.iter().map(|a| (a * 10.0).round() / 10.0).collect::<Vec<_>>(), [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    let across: Vec<f64> = rows.iter().map(|r| (r.2 * 1000.0).round() / 1000.0).collect();
    assert_eq!(across, [0.0, 0.1, -0.1, 0.1, -0.1, 0.0], "0.2 inch from crest to trough, by a fixed length");
}

/// The rows of the first `Geometry` section of a shape: the kind of each row, and its X and Y.
fn zigzag_rows(doc: &Document, page: u32, id: u32) -> Vec<(String, f64, f64)> {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
    let res = Resolver::new(doc);
    let rows = res.effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    rows.iter()
        .map(|row| {
            let at =
                |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap();
            (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
        })
        .collect()
}

fn number(doc: &Document, page: u32, id: u32, cell: &str) -> f64 {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
    Resolver::new(doc)
        .resolve(&sheet, &diacraft_doc::CellAddr::direct(cell))
        .and_then(|f| f.value.cell?.value.as_ref()?.number())
        .unwrap_or_else(|| panic!("no number in {cell}"))
}

#[test]
fn a_comm_links_ends_glue_and_it_keeps_its_zigzag_when_a_device_moves() {
    let mut s = session();
    let router = put(&mut s, "network.router", (2.0, 5.0));
    let switch = put(&mut s, "network.switch", (7.0, 5.0));
    let link = put(&mut s, "network.comm-link", (4.5, 8.0));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "begin", "target": { "id": router } }));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "end", "target": { "id": switch } }));
    assert_eq!(s.document().unwrap().page(1).unwrap().connects.len(), 2, "glued at both ends");
    let before = zigzag_rows(s.document().unwrap(), 1, link);
    let (width, end_x) = (number(s.document().unwrap(), 1, link, "Width"), number(s.document().unwrap(), 1, link, "EndX"));
    run(&mut s, "shape.move", json!({ "ids": [switch], "dx": 1.5, "dy": 0.0 }));
    let doc = s.document().unwrap();
    assert!((number(doc, 1, link, "EndX") - (end_x + 1.5)).abs() < 1e-6, "the end follows the switch");
    let after = zigzag_rows(doc, 1, link);
    let kinds = |rows: &[(String, f64, f64)]| rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    assert_eq!(kinds(&after), kinds(&before), "the same six rows: no route was laid over the zigzag");
    assert_eq!(kinds(&after), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
    let grown = number(doc, 1, link, "Width");
    assert!(grown > width + 1.0, "the zigzag is stretched along the line: {width} to {grown}");
    for (a, b) in after.iter().zip(before.iter()) {
        assert!((a.1 - b.1 * grown / width).abs() < 1e-6, "stretched along the line");
        assert!((a.2 - b.2).abs() < 1e-9, "the same across it: a fifth of an inch");
    }
    // Moved the other way too, and the line ends and pattern stay those of the master (none, solid).
    run(&mut s, "shape.move", json!({ "ids": [switch], "dx": -2.5, "dy": 0.0 }));
    let doc = s.document().unwrap();
    assert_eq!(kinds(&zigzag_rows(doc, 1, link)), kinds(&before));
    assert_eq!([number(doc, 1, link, "BeginArrow"), number(doc, 1, link, "EndArrow")], [0.0, 0.0]);
}

#[test]
fn the_ethernet_bar_has_18_points_in_order() {
    let bar = master("network.ethernet");
    assert_eq!(bar.point_count(), 18);
    let offsets = bar.connection_offsets();
    let (w, h) = (bar.width, bar.height);
    // Eight along the top from the left at the odd sixteenths, the left end, eight along the bottom the same, the right end.
    for k in 0..8 {
        let share = (2 * k + 1) as f64 / 16.0;
        let (top, bottom) = (offsets[k], offsets[9 + k]);
        assert!((top.0 - w * share).abs() < 1e-9 && (top.1 - h).abs() < 1e-9, "top {k}: {top:?}");
        assert!((bottom.0 - w * share).abs() < 1e-9 && bottom.1.abs() < 1e-9, "bottom {k}: {bottom:?}");
    }
    assert!(offsets[8].0.abs() < 1e-9 && (offsets[8].1 - h / 2.0).abs() < 1e-9, "left end: {:?}", offsets[8]);
    assert!((offsets[17].0 - w).abs() < 1e-9 && (offsets[17].1 - h / 2.0).abs() < 1e-9, "right end: {:?}", offsets[17]);
    // Each leaves away from the bar: up along the top, down along the bottom, out at the ends.
    let ways: Vec<(f64, f64)> = bar.points.iter().map(|p| (p.dx, p.dy)).collect();
    for k in 0..8 {
        assert_eq!((ways[k], ways[9 + k]), ((0.0, 1.0), (0.0, -1.0)), "point {k}");
    }
    assert_eq!((ways[8], ways[17]), ((-1.0, 0.0), (1.0, 0.0)));
    // On a page they are in the same order, and each sits on the bar's outline.
    let (doc, page, id) = dropped(bar);
    let points = draw::connection_points(&doc, page, id);
    assert_eq!(points.iter().map(|p| p.index).collect::<Vec<_>>(), (0..18).collect::<Vec<_>>());
    let left = 5.0 - w / 2.0;
    assert!((points[0].x - (left + w / 16.0)).abs() < 1e-6 && (points[17].x - (left + w)).abs() < 1e-6, "{:?} {:?}", points[0], points[17]);
}

// ---- the label and the data -----------------------------------------------------------------------------------------------------------

/// The shown text of the fields of a shape's text, in order, and the characters between them.
fn label(s: &Session, id: u32) -> (Vec<String>, String) {
    let view = draw::text_view(s.document().unwrap(), 1, id).unwrap();
    (view.fields.into_iter().map(|f| f.text).collect(), view.chars)
}

#[test]
fn the_label_shows_the_host_name_over_the_address_and_follows_the_fields() {
    let mut s = session();
    // A new router's first line is its kind, and the address line is empty.
    let router = put(&mut s, "network.router", (3.0, 5.0));
    assert_eq!(label(&s, router), (vec!["Router".to_owned(), String::new()], "\n".to_owned()), "the name over the address, a line break between");
    // A firewall holds the equipment rows only, so it has no network name or address to show: its label is plain text that starts as its kind.
    let wall = put(&mut s, "network.firewall", (5.0, 5.0));
    assert_eq!(label(&s, wall), (Vec::<String>::new(), "Firewall".to_owned()), "the shape's own text, so that every format reads it back the same");
    // A switch with its network name and address typed in the Shape Data window shows both, the name over the address.
    let switch = put(&mut s, "network.switch", (7.0, 5.0));
    run(
        &mut s,
        "shapedata.set",
        json!({ "id": switch, "values": [{ "row": "NetworkName", "value": "core-sw-01" }, { "row": "IPAddress", "value": "10.20.30.40" }] }),
    );
    assert_eq!(label(&s, switch), (vec!["core-sw-01".to_owned(), "10.20.30.40".to_owned()], "\n".to_owned()));
    // Another row of the data is not on the label.
    run(&mut s, "shapedata.set", json!({ "id": switch, "row": "Location", "value": "Rack 4" }));
    assert_eq!(label(&s, switch).0, ["core-sw-01", "10.20.30.40"]);
    // Clearing the address leaves the first line and nothing on the second.
    run(&mut s, "shapedata.set", json!({ "id": switch, "row": "IPAddress", "value": "" }));
    assert_eq!(label(&s, switch).0, ["core-sw-01", ""]);
    // The label is two fields of the master's text: the first the name, the second the address.
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), switch).unwrap();
    let master = doc.master(doc.shape(&at).unwrap().master.unwrap()).unwrap();
    let formulas: Vec<String> = master
        .shapes
        .get(0)
        .unwrap()
        .sheet
        .sections
        .iter()
        .filter(|sec| sec.name.as_str() == "Field")
        .flat_map(|sec| sec.rows.iter())
        .filter_map(|r| match &r.cells.get("Value")?.formula {
            diacraft_doc::FormulaSlot::Local(f) => Some(f.source.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(formulas, ["Prop.NetworkName", "Prop.IPAddress"]);
}

#[test]
fn every_devices_label_starts_as_its_kind_below_the_figure_and_flowing_down_from_it() {
    all_present();
    for (_, m) in devices() {
        let (doc, page, id) = dropped(m);
        let view = draw::text_view(&doc, page, id).unwrap();
        if m.id == "network.firewall" {
            // The firewall holds no network rows, so its label is text of its own, which starts as its kind.
            assert_eq!((view.chars.as_str(), view.fields.len()), ("Firewall", 0), "{}: plain text of its own", m.id);
        } else {
            assert_eq!(view.fields.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), [m.name, ""], "{}: the name over an empty address", m.id);
        }
        // The text block is below the figure (its top at or under the bottom edge) and the text flows down from its top, so a second line never climbs onto the figure.
        let top_of_block = number(&doc, page, id, "TxtPinY") + number(&doc, page, id, "TxtHeight") - number(&doc, page, id, "TxtLocPinY");
        assert!(top_of_block <= 1e-9, "{}: the label's block starts at {top_of_block} (the figure's bottom is 0)", m.id);
        assert_eq!(number(&doc, page, id, "VerticalAlign"), 0.0, "{}: the label is top aligned", m.id);
        // Its place is a formula of the shape's own width and height, so it follows the figure.
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap());
        let found = Resolver::new(&doc).resolve(&sheet, &diacraft_doc::CellAddr::direct("TxtPinX")).unwrap();
        assert!(matches!(found.value.cell.map(|c| &c.formula), Some(diacraft_doc::FormulaSlot::Local(_))), "{}: TxtPinX is a formula", m.id);
    }
}

const EQUIPMENT: [(&str, &str); 9] = [
    ("Manufacturer", "Manufacturer"),
    ("ProductNumber", "Product Number"),
    ("PartNumber", "Part Number"),
    ("ProductDescription", "Product Description"),
    ("AssetNumber", "Asset Number"),
    ("SerialNumber", "Serial Number"),
    ("Location", "Location"),
    ("Building", "Building"),
    ("Room", "Room"),
];
const NETWORKED: [(&str, &str); 8] = [
    ("NetworkName", "Network Name"),
    ("IPAddress", "IP Address"),
    ("SubnetMask", "Subnet Mask"),
    ("MACAddress", "MAC Address"),
    ("AdminInterface", "Admin Interface"),
    ("NumberofPorts", "Number of Ports"),
    ("CommunityString", "Community String"),
    ("NetworkDescription", "Network Description"),
];
const COMPUTER: [(&str, &str); 4] =
    [("CPU", "CPU"), ("Memory", "Memory"), ("OperatingSystem", "Operating System"), ("HardDriveSize", "Hard Drive Size")];
const DEPARTMENT: (&str, &str) = ("Department", "Department");

/// The computers that hold the computer group: the server and its six badged towers, the rack server, the desktop, the laptop and the tablet.
const COMPUTER_GROUP: [&str; 11] = [
    "computers.server",
    "computers.database-server",
    "computers.web-server",
    "computers.file-server",
    "computers.mail-server",
    "computers.application-server",
    "computers.print-server",
    "computers.rack-server",
    "computers.desktop",
    "computers.laptop",
    "computers.tablet",
];

/// The rows say a master holds, in order, as (row, label).
fn wanted_rows(id: &str) -> Vec<(&'static str, &'static str)> {
    let mut rows: Vec<(&str, &str)> = EQUIPMENT.to_vec();
    if id != "network.firewall" {
        rows.extend(NETWORKED);
    }
    if COMPUTER_GROUP.contains(&id) {
        rows.extend(COMPUTER);
    }
    // `Department` is on the computers and, as the samples show, on the backbones.
    if COMPUTER_GROUP.contains(&id) || ["network.ethernet", "network.ring-network"].contains(&id) {
        rows.push(DEPARTMENT);
    }
    rows
}

#[test]
fn every_device_has_the_equipment_fields_and_each_group_of_the_reference_apps_rows_where_it_belongs() {
    // A router has the equipment and network rows and no computer rows; a server has all three groups and Department; a firewall has the equipment rows only.
    all_present();
    for (_, m) in devices() {
        let mut s = session();
        let id = put(&mut s, m.id, (4.0, 5.0));
        let listed = run(&mut s, "shapedata.list", json!({ "id": id }));
        let fields = listed["fields"].as_array().unwrap();
        let got: Vec<(&str, &str)> = fields.iter().map(|f| (f["row"].as_str().unwrap(), f["label"].as_str().unwrap())).collect();
        assert_eq!(got, wanted_rows(m.id), "{}", m.id);
        for f in fields {
            let row = f["row"].as_str().unwrap();
            assert_eq!(f["type"], "string", "{}: {row} is text", m.id);
            let want = if row == "NetworkName" { m.name } else { "" };
            assert_eq!(f["value"].as_str().unwrap_or(""), want, "{}: {row} starts as {want:?}", m.id);
        }
    }
    assert_eq!(wanted_rows("network.router").len(), 17);
    assert_eq!(wanted_rows("network.firewall").len(), 9);
    assert_eq!(wanted_rows("computers.server").len(), 22);
    // A line holds no device data: it is no device.
    for id in ["network.comm-link", "network.link"] {
        let mut s = session();
        let line = put(&mut s, id, (4.0, 5.0));
        let listed = run(&mut s, "shapedata.list", json!({ "id": line }));
        assert!(listed["fields"].as_array().unwrap().is_empty(), "{id}: {listed}");
    }
}

// ---- the Quick Shapes, the search and the template -------------------------------------------------------------------------------

/// The ids of a stencil's Quick Shapes, as `stencil.quick` gives them.
fn quick(s: &mut Session, stencil: &str) -> Vec<String> {
    let answer = run(s, "stencil.quick", json!({ "stencil": stencil }));
    answer["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn the_quick_shapes_are_router_switch_firewall_cloud_and_server_desktop_laptop_database_server() {
    let mut s = session();
    assert_eq!(quick(&mut s, "network"), ["network.router", "network.switch", "network.firewall", "network.cloud"]);
    assert_eq!(quick(&mut s, "computers"), ["computers.server", "computers.desktop", "computers.laptop", "computers.database-server"]);
    // The two lines are no Quick Shapes and the strip offers none of them.
    let answer = run(&mut s, "stencil.quick", json!({ "stencil": "network" }));
    let offered: Vec<&str> = answer["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(offered, ["network.router", "network.switch", "network.firewall", "network.cloud"]);
}

/// The ids a search for `text` finds.
fn found(s: &mut Session, text: &str) -> Vec<String> {
    let answer = run(s, "stencil.search", json!({ "text": text }));
    answer["results"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["masters"].as_array().unwrap().iter())
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn every_master_has_search_words_and_c32s_names_find_their_masters() {
    for (_, m) in all() {
        assert!(!crate::stencil::keywords::of(m.id).is_empty(), "{} has no search words", m.id);
    }
    let mut s = session();
    // The names of the inventory's sample list (C32), and the master that serves each. A name finds its master, and a name that is one master's alone finds only it.
    let by_name: [(&str, &str, bool); 17] = [
        ("Server", "computers.server", false),
        ("Database server", "computers.database-server", true),
        ("Ethernet", "network.ethernet", false),
        ("PC", "computers.desktop", true),
        ("Router", "network.router", true),
        ("Firewall", "network.firewall", true),
        ("Printer", "computers.printer", false),
        ("Web server", "computers.web-server", true),
        ("Application server", "computers.application-server", true),
        ("Cloud", "network.cloud", false),
        ("Laptop", "computers.laptop", true),
        ("Switch", "network.switch", true),
        ("File server", "computers.file-server", true),
        ("Mail server", "computers.mail-server", true),
        ("Print server", "computers.print-server", true),
        ("Wireless access point", "network.wireless-access-point", true),
        ("Comm-link", "network.comm-link", true),
    ];
    for (name, id, only) in by_name {
        let hits = found(&mut s, name);
        assert!(hits.iter().any(|h| h == id), "`{name}` does not find {id}: {hits:?}");
        if only {
            assert_eq!(hits, [id], "`{name}` finds {id} alone");
        }
    }
}

#[test]
fn no_master_or_search_word_names_a_vendor_or_a_product() {
    all_present();
    // The names are generic. The words a vendor's marks and products are known by are in no id, name or search word.
    const VENDORS: [&str; 24] = [
        "cisco", "juniper", "aruba", "netgear", "ubiquiti", "linksys", "fortinet", "paloalto", "dell", "hp", "ibm", "lenovo", "apple", "iphone",
        "ipad", "macbook", "android", "windows", "linux", "aws", "azure", "google", "vmware", "oracle",
    ];
    for (_, m) in all() {
        let text = format!("{} {} {}", m.id, m.name, crate::stencil::keywords::of(m.id)).to_ascii_lowercase();
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            assert!(!VENDORS.contains(&word), "{}: `{word}` is a vendor's or a product's name", m.id);
        }
    }
}

#[test]
fn the_basic_network_template_is_listed_and_opens_the_network_the_computers_and_the_basic_shapes() {
    let template = crate::template::find("basic_network").expect("both stencils ship, so the template is listed");
    assert_eq!((template.name, template.stencils), ("Basic Network Diagram", &["network", "computers", "basic"][..]));
    // The description names the groups of Shape Data the devices hold now, not the design's five fields.
    for stale in ["host name", "asset tag", "notes"] {
        assert!(!template.description.contains(stale), "the description still says `{stale}`: {}", template.description);
    }
    assert!(template.description.contains("based on"), "{}", template.description);
    let mut s = Session::new();
    let started = run(&mut s, "file.new", json!({ "template": "basic_network" }));
    assert_eq!(started["stencils"]["open"], json!(["network", "computers", "basic"]), "{started}");
    let listed = run(&mut s, "template.list", json!({}));
    assert!(listed["templates"].as_array().unwrap().iter().any(|t| t["id"] == "basic_network"));
}

// ---- the glyph kit -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_outer_crossing_of_two_circles_is_the_one_farther_from_the_cloud() {
    // Circles of radius 5 at (0, 0) and (6, 0) cross at (3, 4) and (3, -4).
    let (a, b) = ((0.0, 0.0, 5.0), (6.0, 0.0, 5.0));
    let up = glyphs::outer_intersection(a, b, (3.0, -10.0)).expect("they cross");
    assert!((up.0 - 3.0).abs() < 1e-9 && (up.1 - 4.0).abs() < 1e-9, "{up:?}");
    let down = glyphs::outer_intersection(a, b, (3.0, 10.0)).expect("they cross");
    assert!((down.0 - 3.0).abs() < 1e-9 && (down.1 + 4.0).abs() < 1e-9, "{down:?}");
    // Circles that do not meet, one inside the other, and one centre twice, cross nowhere.
    assert_eq!(glyphs::outer_intersection((0.0, 0.0, 1.0), (5.0, 0.0, 1.0), (0.0, 0.0)), None);
    assert_eq!(glyphs::outer_intersection((0.0, 0.0, 5.0), (1.0, 0.0, 1.0), (0.0, 0.0)), None);
    assert_eq!(glyphs::outer_intersection((0.0, 0.0, 1.0), (0.0, 0.0, 1.0), (0.0, 0.0)), None);
    assert_eq!(glyphs::outer_intersection((0.0, 0.0, f64::NAN), (1.0, 0.0, 1.0), (0.0, 0.0)), None, "a number that is no number crosses nothing");
}

// ---- what is drawn over what ---------------------------------------------------------------------------------------------------------

const PPI: f64 = 200.0;

/// The page drawn at 200 pixels to the inch, on its own (no paper).
fn render(doc: &Document, page: u32) -> diacraft_render::Rendered {
    let view = diacraft_render::View::fit_page(doc, page, PPI).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    renderer.render(doc, page, &view)
}

/// Whether there is ink within two pixels of a place on the page (inches from the left and bottom edges).
fn ink_near(r: &diacraft_render::Rendered, height: f64, (x, y): (f64, f64)) -> bool {
    let (col, row) = ((x * PPI) as i64, ((height - y) * PPI) as i64);
    (-2..=2).any(|dy| {
        (-2..=2).any(|dx| {
            let (c, rw) = (usize::try_from(col + dx).unwrap(), usize::try_from(row + dy).unwrap());
            let at = (rw.min(r.height as usize - 1) * r.width as usize + c.min(r.width as usize - 1)) * 4;
            let p = &r.rgba_premul[at..at + 4];
            p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
        })
    })
}

#[test]
fn a_badge_hides_the_towers_edge_behind_it_and_the_printers_sheet_ends_at_its_body() {
    // A shape has one fill and one line, so a section drawn over another does not hide its line: a badge over the tower's edge would show the edge through it.
    for id in [
        "computers.database-server",
        "computers.web-server",
        "computers.file-server",
        "computers.mail-server",
        "computers.application-server",
        "computers.print-server",
    ] {
        let m = master(id);
        let (doc, page, _) = dropped(m);
        let height = doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
        let drawn = render(&doc, page);
        let (left, bottom) = (5.0 - m.width / 2.0, 5.0 - m.height / 2.0);
        // The tower is 0.56 inch wide, and its right edge is drawn above the badge and below it, but not through it (0.12 to 0.56 up).
        assert!(ink_near(&drawn, height, (left + 0.56, bottom + 0.06)), "{id}: the edge below the badge");
        assert!(ink_near(&drawn, height, (left + 0.56, bottom + 0.80)), "{id}: the edge above the badge");
        assert!(!ink_near(&drawn, height, (left + 0.56, bottom + 0.15)), "{id}: the tower's edge shows through the badge");
        assert!(ink_near(&drawn, height, (left + 0.80, bottom + 0.34)), "{id}: the badge's own side");
    }
    // The printer's sheet starts where the body ends: its sides are not drawn down through the body.
    let m = master("computers.printer");
    let (doc, page, _) = dropped(m);
    let height = doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
    let drawn = render(&doc, page);
    let (left, bottom) = (5.0 - m.width / 2.0, 5.0 - m.height / 2.0);
    assert!(ink_near(&drawn, height, (left + 0.22, bottom + 0.55)), "the sheet's side above the body");
    assert!(!ink_near(&drawn, height, (left + 0.22, bottom + 0.36)), "the sheet's side shows through the body");
}
