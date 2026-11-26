//! Computers and Devices (`computers`, GUID series 18): sixteen masters drawn in code (design section 6.4).
//!
//! **Original figures.** Each is a pictogram made by the functions of `network::glyphs` (the glyph kit Network Devices shares), with the label under it. A server of a role is a tower with a
//! small badge of the role (a cylinder, a globe, a folder, an envelope, a window, a printer): generic metaphors, no vendor's mark. The sample names of the inventory's C32 (Server, Database
//! server, PC, Laptop, Printer, Web server, Application server, File server, Mail server, Print server) are search words of the master that serves each purpose.
//!
//! * **The order** is the Shapes window's, and the Quick Shapes are the first four: Server, Desktop, Laptop, Database Server.
//! * **The Shape Data** is the reference application's rows in its groups (`network::fields`): every device the equipment and the network group; the servers (all seven), the rack server,
//!   the desktop, the laptop and the tablet the computer group and `Department` too. The label shows `NetworkName` over `IPAddress`, and `NetworkName` starts as the device's kind.
//! * **The points** are the middles of the sides of the box ([`SIDES`]) where the outline reaches them: the laptop's lid and the user's shoulders have them at the middle of the part that
//!   does, the telephone's handset rests on its base at the box's middle height.

use super::kit;
use super::network::fields::device;
use super::network::glyphs::{self, Pane};
use super::{MasterSpec, More, Outline, OwnedPath, Point, SIDES, pt};

/// The series of the stencil's masters in their GUIDs (`D1AC0012-...`).
const SERIES: u8 = 18;

// The statics of the devices. A computer holds the computer group; a phone, a telephone, a printer, storage and a user do not.
device!(server, "Server", Computer);
device!(desktop, "Desktop", Computer);
device!(laptop, "Laptop", Computer);
device!(database_server, "Database Server", Computer);
device!(web_server, "Web Server", Computer);
device!(file_server, "File Server", Computer);
device!(mail_server, "Mail Server", Computer);
device!(application_server, "Application Server", Computer);
device!(print_server, "Print Server", Computer);
device!(rack_server, "Rack Server", Computer);
device!(tablet, "Tablet", Computer);
device!(mobile_phone, "Mobile Phone", Networked);
device!(telephone, "Telephone", Networked);
device!(printer, "Printer", Networked);
device!(storage, "Storage", Networked);
device!(user, "User", Networked);

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

/// The laptop's places: the lid's top, the lid's sides at half the box's height (the lid is a little narrower than the base), and the base's front edge.
static LAPTOP_POINTS: [Point; 4] =
    [pt(0.5, 1.0, 0.0, 1.0), pt(0.127_272_727, 0.666_666_667, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.872_727_273, 0.666_666_667, 1.0, 0.0)];

/// The user's places: the top of the head, the shoulders' sides low down (the head and the shoulders are narrower than the box at its middle), and the shoulders' foot.
static USER_POINTS: [Point; 4] =
    [pt(0.5, 1.0, 0.0, 1.0), pt(0.033_333_333, 0.1, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.966_666_667, 0.1, 1.0, 0.0)];

pub(super) static MASTERS: [MasterSpec; 16] = [
    device_master(1, "computers.server", "Server", glyphs::SERVER, glyphs::server, &SIDES, &server::MORE),
    device_master(2, "computers.desktop", "Desktop", glyphs::DESKTOP, glyphs::desktop, &SIDES, &desktop::MORE),
    device_master(3, "computers.laptop", "Laptop", glyphs::LAPTOP, glyphs::laptop, &LAPTOP_POINTS, &laptop::MORE),
    device_master(4, "computers.database-server", "Database Server", glyphs::TOWER, glyphs::database_server, &SIDES, &database_server::MORE),
    device_master(5, "computers.web-server", "Web Server", glyphs::TOWER, glyphs::web_server, &SIDES, &web_server::MORE),
    device_master(6, "computers.file-server", "File Server", glyphs::TOWER, glyphs::file_server, &SIDES, &file_server::MORE),
    device_master(7, "computers.mail-server", "Mail Server", glyphs::TOWER, glyphs::mail_server, &SIDES, &mail_server::MORE),
    device_master(
        8,
        "computers.application-server",
        "Application Server",
        glyphs::TOWER,
        glyphs::application_server,
        &SIDES,
        &application_server::MORE,
    ),
    device_master(9, "computers.print-server", "Print Server", glyphs::TOWER, glyphs::print_server, &SIDES, &print_server::MORE),
    device_master(10, "computers.rack-server", "Rack Server", glyphs::RACK, glyphs::rack_server, &SIDES, &rack_server::MORE),
    device_master(11, "computers.tablet", "Tablet", glyphs::TABLET, glyphs::tablet, &SIDES, &tablet::MORE),
    device_master(12, "computers.mobile-phone", "Mobile Phone", glyphs::PHONE, glyphs::mobile_phone, &SIDES, &mobile_phone::MORE),
    device_master(13, "computers.telephone", "Telephone", glyphs::TELEPHONE, glyphs::telephone, &SIDES, &telephone::MORE),
    device_master(14, "computers.printer", "Printer", glyphs::PRINTER, glyphs::printer, &SIDES, &printer::MORE),
    device_master(15, "computers.storage", "Storage", glyphs::STORAGE, glyphs::storage, &SIDES, &storage::MORE),
    device_master(16, "computers.user", "User", glyphs::USER, glyphs::user, &USER_POINTS, &user::MORE),
];

/// The search words of the master with this id, space separated, or `""`. Plain English: what the figure is called elsewhere and what it is used for, and the names of the
/// inventory's sample list that its purpose serves ("PC" is the desktop).
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "computers.server" => "host machine tower",
        "computers.desktop" => "pc personal computer workstation monitor screen",
        "computers.laptop" => "notebook portable computer",
        "computers.database-server" => "db sql data storage",
        "computers.web-server" => "http www site",
        "computers.file-server" => "files share",
        "computers.mail-server" => "email smtp imap messaging",
        "computers.application-server" => "app middleware",
        "computers.print-server" => "printing queue",
        "computers.rack-server" => "rack mount datacentre datacenter blade",
        "computers.tablet" => "slate touch pad",
        "computers.mobile-phone" => "smartphone cell handset mobile",
        "computers.telephone" => "phone desk voip handset",
        "computers.printer" => "print paper office",
        "computers.storage" => "disk array drives san nas",
        "computers.user" => "person people client end account",
        _ => "",
    }
}
