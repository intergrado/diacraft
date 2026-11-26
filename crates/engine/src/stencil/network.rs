//! Network Devices (`network`, GUID series 17): twelve masters drawn in code (section 6.4 of the second stencil batch's design, "Stencils: UML, BPMN, C4, network and database", and of the
//! batch's notation spec, which win where they differ).
//!
//! **Original figures.** Each device is a pictogram of rectangles, circles, arcs and strokes made by the functions of [`glyphs`], with the label under it ([`kit::LABEL_BELOW`]). The names are the
//! generic ones of the inventory's sample list (router, switch, firewall, cloud, wireless access point, ethernet, comm-link) and no vendor's mark or icon set is used.
//!
//! * **The order** is the Shapes window's, and the Quick Shapes are the first four that are not lines: Router, Switch, Firewall, Cloud.
//! * **The Shape Data** is the reference application's rows in its groups ([`fields`]): every device the equipment group, every one but the firewall the network group, the backbones
//!   `Department` too. The label shows `NetworkName` over `IPAddress`, and `NetworkName` starts as the device's kind. The two lines hold no device data.
//! * **The points** are the middles of the box's sides ([`SIDES`]), on the outline, where the outline reaches them: the cloud and the access point have the four places their outline does reach
//!   at the middle of the body, the globe and the ring the eight of [`RING8`], and the backbone **eighteen**: eight along the top, one at the left end, eight along the bottom and one at the right
//!   end (the backbone's arms wait for control handles, M4).
//! * **The two lines.** The Comm-link is a [`Outline::Fixed1D`] zigzag of five legs, 0.2 inch across the line: its ends glue and a move of a device stretches it and never routes it. The Network
//!   Link is an ordinary connector with no ends, in a solid right-angle route.

use super::kit::{self, RING8};
use super::{ConnectorLook, Flag, LineEnd, MasterSpec, More, Outline, OwnedPath, PathSpec, Point, Route, SIDES, Seg, TextBox, fh, fk, fw, pt};

pub(super) mod fields;
pub(super) mod glyphs;
#[cfg(test)]
mod tests;

use fields::device;
use glyphs::Pane;

/// The series of the stencil's masters in their GUIDs (`D1AC0011-...`).
const SERIES: u8 = 17;

// The statics of the devices: `router::MORE` and so on. Every device but the firewall is networked, and the backbones hold `Department` too.
device!(router, "Router", Networked);
device!(switch, "Switch", Networked);
device!(firewall, "Firewall", Equipment);
device!(cloud, "Cloud", Networked);
device!(access_point, "Wireless Access Point", Networked);
device!(load_balancer, "Load Balancer", Networked);
device!(modem, "Modem", Networked);
device!(internet, "Internet", Networked);
device!(ethernet, "Ethernet", Backbone, { fill: Some("THEMEVAL(\"Dark\",RGB(0,0,0))"), });
device!(ring, "Ring Network", Backbone, { line_weight_pt: Some(5.0), });

/// A device: a glyph in its icon box with its label under it.
const fn device_master(
    at: u16,
    id: &'static str,
    name: &'static str,
    pane: Pane,
    build: fn() -> Vec<OwnedPath>,
    points: &'static [Point],
    more: &'static More,
) -> MasterSpec {
    MasterSpec {
        identity: (SERIES, at),
        id,
        name,
        width: pane.w,
        height: pane.h,
        outline: Outline::Code { build, text: kit::LABEL_BELOW },
        points,
        more,
    }
}

/// The cloud's four places: the top of its middle lobe, the base, and the outer points of its two end lobes, a third of the way up.
static CLOUD_POINTS: [Point; 4] =
    [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.333_333_333, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.333_333_333, 1.0, 0.0)];

/// The access point's four places: the top of its outer signal arc, and the middles of the sides of its body, with the bottom.
static ACCESS_POINT_POINTS: [Point; 4] = [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.1875, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.1875, 1.0, 0.0)];

/// Eight points along the top at the odd sixteenths, the left end, eight along the bottom, the right end: the bar's eighteen (the shares are exact in decimals, so a cell's value is its formula's).
static BUS: [Point; 18] = bus();

const fn bus() -> [Point; 18] {
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); 18];
    let mut i = 0;
    while i < 8 {
        let share = (2 * i + 1) as f64 / 16.0;
        out[i] = pt(share, 1.0, 0.0, 1.0);
        out[9 + i] = pt(share, 0.0, 0.0, -1.0);
        i += 1;
    }
    out[8] = pt(0.0, 0.5, -1.0, 0.0);
    out[17] = pt(1.0, 0.5, 1.0, 0.0);
    out
}

const Z: super::Co = fw(0.0);
const W: super::Co = fw(1.0);
const H: super::Co = fh(1.0);

/// The bar: one filled rectangle.
static BAR: [PathSpec; 1] = [PathSpec {
    fill: true,
    segs: &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

/// The bar's label: a block as wide as the bar and 0.336 inch tall, from 0.048 inch under it to 0.384 inch (two and four fifths of its height, from two fifths of its height below it).
const BAR_LABEL: TextBox = TextBox { x: 0.0, y: -3.2, w: 1.0, h: 2.8 };

/// The ring: an ellipse with no fill, drawn heavy (the dress's line weight).
static RING_PATH: [PathSpec; 1] = [PathSpec { fill: false, segs: &[Seg::Ellipse], no_show: Flag::Off, no_line: Flag::Off }];

/// The Comm-link's zigzag: five legs, 0.1 inch to each side of the line, by fixed lengths.
static ZIGZAG: [PathSpec; 1] = [PathSpec {
    fill: false,
    segs: &[
        Seg::Move(Z, Z),
        Seg::Line(fw(0.2), fk(0.1)),
        Seg::Line(fw(0.4), fk(-0.1)),
        Seg::Line(fw(0.6), fk(0.1)),
        Seg::Line(fw(0.8), fk(-0.1)),
        Seg::Line(W, Z),
    ],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

/// The Network Link: no ends, solid, right-angled.
const LINK_LOOK: ConnectorLook = ConnectorLook { route: Route::RightAngle, ..ConnectorLook::plain(LineEnd::None, LineEnd::None) };

pub(super) static MASTERS: [MasterSpec; 12] = [
    device_master(1, "network.router", "Router", glyphs::ROUTER, glyphs::router, &SIDES, &router::MORE),
    device_master(2, "network.switch", "Switch", glyphs::SWITCH, glyphs::switch, &SIDES, &switch::MORE),
    device_master(3, "network.firewall", "Firewall", glyphs::FIREWALL, glyphs::firewall, &SIDES, &firewall::MORE),
    device_master(4, "network.cloud", "Cloud", glyphs::CLOUD, glyphs::cloud, &CLOUD_POINTS, &cloud::MORE),
    device_master(
        5,
        "network.wireless-access-point",
        "Wireless Access Point",
        glyphs::ACCESS_POINT,
        glyphs::access_point,
        &ACCESS_POINT_POINTS,
        &access_point::MORE,
    ),
    device_master(6, "network.load-balancer", "Load Balancer", glyphs::BALANCER, glyphs::load_balancer, &SIDES, &load_balancer::MORE),
    device_master(7, "network.modem", "Modem", glyphs::MODEM, glyphs::modem, &SIDES, &modem::MORE),
    device_master(8, "network.internet", "Internet", glyphs::INTERNET, glyphs::internet, &RING8, &internet::MORE),
    MasterSpec {
        identity: (SERIES, 9),
        id: "network.ethernet",
        name: "Ethernet",
        width: glyphs::ETHERNET.w,
        height: glyphs::ETHERNET.h,
        outline: Outline::Paths { paths: &BAR, text: BAR_LABEL },
        points: &BUS,
        more: &ethernet::MORE,
    },
    MasterSpec {
        identity: (SERIES, 10),
        id: "network.ring-network",
        name: "Ring Network",
        width: glyphs::RING.w,
        height: glyphs::RING.h,
        outline: Outline::Paths { paths: &RING_PATH, text: kit::LABEL_BELOW },
        points: &RING8,
        more: &ring::MORE,
    },
    MasterSpec {
        identity: (SERIES, 11),
        id: "network.comm-link",
        name: "Comm-link",
        width: 1.5,
        height: 0.0,
        outline: Outline::Fixed1D { look: ConnectorLook::plain(LineEnd::None, LineEnd::None), paths: &ZIGZAG },
        points: &[],
        more: &More::NONE,
    },
    MasterSpec {
        identity: (SERIES, 12),
        id: "network.link",
        name: "Network Link",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(LINK_LOOK),
        points: &[],
        more: &More::NONE,
    },
];

/// The search words of the master with this id, space separated, or `""`. Plain English: what the figure is called elsewhere and what it is used for, and the names of the
/// inventory's sample list that its purpose serves.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "network.router" => "gateway routing layer network device",
        "network.switch" => "lan layer network device hub",
        "network.firewall" => "security wall barrier filter protection",
        "network.cloud" => "internet wan remote hosted service network",
        "network.wireless-access-point" => "wifi wi fi wlan wireless ap hotspot radio antenna",
        "network.load-balancer" => "balancer traffic distribute spread",
        "network.modem" => "broadband dsl cable connection",
        "network.internet" => "web globe world wan",
        "network.ethernet" => "backbone bus segment cable wired",
        "network.ring-network" => "backbone token loop topology",
        "network.comm-link" => "wireless link radio signal communication lightning",
        "network.link" => "connection line cable wire",
        _ => "",
    }
}
