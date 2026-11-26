//! The Shape Data and the label of a device:.
//!
//! **The rows are those of the reference application's device masters**, by name, in its three groups: *equipment* on every device (`Manufacturer` to `Room`), *network* on every networked
//! device (`NetworkName` to `NetworkDescription`: every network device and computer but the firewall, and every cloud service), and *computer* on the computers (`CPU` to `HardDriveSize`),
//! with `Department` after them on the computers and, as the samples show, on the backbones. The labels are ours: the row name in spaced words. The order of the rows is ours (the groups as
//! the spec lists them), every row is text, and every row starts empty but `NetworkName`, which starts as the device's kind.
//!
//! **The label** is two fields of the master's text, `NetworkName` over `IPAddress`: a person types them in the Shape Data window and the text follows. The text sits below the
//! figure and flows down from the top of its block, so a second line never climbs onto the figure. **A firewall holds the equipment group only**, so it has no such rows to show: its label is text
//! of its own that starts as `Firewall`, which a person types over.
//!
//! A master's four statics (its rows, its text, its dress and its extras) are made by the `device!` macro below, from the groups it holds and its kind. The rows are built at compile time, and a
//! test holds each group's count to the rows it holds. Cloud Services calls the same pieces for its services.

use crate::stencil::dress::{CharRow, Dress, FieldDef, FieldKind, FieldText, TextSpec};

/// A text row of Shape Data that starts empty.
const fn row(name: &'static str, label: &'static str, prompt: &'static str) -> FieldDef {
    FieldDef { row: name, label, kind: FieldKind::String, value: "", prompt }
}

/// Every row a device may hold, in the order the Shape Data window lists them: equipment (0 to 8), network (9 to 16), computer (17 to 20) and `Department` (21).
const ROWS: [FieldDef; 22] = [
    row("Manufacturer", "Manufacturer", "Who makes the device"),
    row("ProductNumber", "Product Number", "The maker's product number"),
    row("PartNumber", "Part Number", "The part number"),
    row("ProductDescription", "Product Description", "What the product is"),
    row("AssetNumber", "Asset Number", "The asset number on the device's tag"),
    row("SerialNumber", "Serial Number", "The serial number"),
    row("Location", "Location", "Where the device is"),
    row("Building", "Building", "The building it is in"),
    row("Room", "Room", "The room it is in"),
    row("NetworkName", "Network Name", "The device's name on the network"),
    row("IPAddress", "IP Address", "The device's address on the network"),
    row("SubnetMask", "Subnet Mask", "The subnet mask"),
    row("MACAddress", "MAC Address", "The hardware address"),
    row("AdminInterface", "Admin Interface", "How the device is managed"),
    row("NumberofPorts", "Number of Ports", "How many ports the device has"),
    row("CommunityString", "Community String", "The management community string"),
    row("NetworkDescription", "Network Description", "What the device does on the network"),
    row("CPU", "CPU", "The processor"),
    row("Memory", "Memory", "How much memory"),
    row("OperatingSystem", "Operating System", "The operating system"),
    row("HardDriveSize", "Hard Drive Size", "The size of the hard drive"),
    row("Department", "Department", "The department that uses the device"),
];

/// The index of `NetworkName` in [`ROWS`]: the one row that does not start empty.
const NETWORK_NAME: usize = 9;

/// Which groups of rows a device holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::stencil) enum Groups {
    /// The equipment group only: the firewall.
    Equipment,
    /// Equipment and network: a network device, a printer, a phone, storage, a user.
    Networked,
    /// Equipment, network and `Department`: a backbone.
    Backbone,
    /// Equipment, network, computer and `Department`: a computer.
    Computer,
}

impl Groups {
    /// How many rows the device holds.
    pub(in crate::stencil) const fn count(self) -> usize {
        match self {
            Groups::Equipment => 9,
            Groups::Networked => 17,
            Groups::Backbone => 18,
            Groups::Computer => 22,
        }
    }

    /// Whether the label shows `NetworkName` over `IPAddress`: the device holds those rows. A firewall holds the equipment group only, so its label is text of its own.
    pub(in crate::stencil) const fn labelled(self) -> bool {
        !matches!(self, Groups::Equipment)
    }

    /// Whether the device holds row `at` of [`ROWS`].
    const fn holds(self, at: usize) -> bool {
        match self {
            Groups::Equipment => at < 9,
            Groups::Networked => at < 17,
            Groups::Backbone => at < 17 || at == 21,
            Groups::Computer => true,
        }
    }
}

/// The rows a device of these groups holds, in order, with `NetworkName` starting as `kind`. `N` is [`Groups::count`], which the `device!` macro passes.
pub(in crate::stencil) const fn data<const N: usize>(kind: &'static str, groups: Groups) -> [FieldDef; N] {
    let mut out = [ROWS[0]; N];
    let (mut from, mut into) = (0, 0);
    while from < ROWS.len() {
        if groups.holds(from) && into < N {
            let mut chosen = ROWS[from];
            if from == NETWORK_NAME {
                chosen.value = kind;
            }
            out[into] = chosen;
            into += 1;
        }
        from += 1;
    }
    out
}

/// The label: `NetworkName` over `IPAddress`, the name's shown form starting as `kind` and the address's empty.
pub(in crate::stencil) const fn label(kind: &'static str) -> [FieldText; 2] {
    [
        FieldText { formula: "Prop.NetworkName", shown: kind, char_row: 0, line_after: true },
        FieldText { formula: "Prop.IPAddress", shown: "", char_row: 1, line_after: false },
    ]
}

/// The name at 9 points and the address at 8, in the style's colour.
static LABEL_CHARS: [CharRow; 2] = [CharRow { size_pt: 9.0, style: 0, color: None }, CharRow { size_pt: 8.0, style: 0, color: None }];

/// `VerticalAlign` 0: the text flows down from the top of its block. The block is below the figure, and two lines of text would otherwise be centred on it and climb onto the figure. (A dress
/// has no field for a text block's cells, so the cell goes in with the protection cells, which are written as plain numbers.)
static TOP_ALIGNED: [(&str, f64); 1] = [("VerticalAlign", 0.0)];

/// The dress of a device: its Shape Data, the label (over two fields of the data, or for a firewall, which holds no network rows, plain text that starts as `kind`), `ObjType` 1 (a shape people
/// join) and the text top aligned.
pub(in crate::stencil) const fn device_dress(groups: Groups, kind: &'static str, data: &'static [FieldDef], text: &'static [FieldText]) -> Dress {
    let label = if groups.labelled() { TextSpec::Fields(text) } else { TextSpec::Plain(kind) };
    Dress { text: label, chars: &LABEL_CHARS, data, locks: &TOP_ALIGNED, obj_type: Some(1), ..Dress::NONE }
}

/// Makes the statics of a device inside a module of its own: `module::MORE` is the master's extras. `kind` is the text the name starts as, `groups` a variant of [`Groups`], and the braces,
/// when there are any, are fields of the dress that differ from [`device_dress`] (a fill, a line weight).
macro_rules! device {
    ($module:ident, $kind:literal, $groups:ident) => {
        $crate::stencil::network::fields::device!($module, $kind, $groups, {});
    };
    ($module:ident, $kind:literal, $groups:ident, { $($over:tt)* }) => {
        mod $module {
            use $crate::stencil::dress::{Dress, FieldDef, FieldText, More};
            use $crate::stencil::network::fields::{self, Groups};

            const GROUPS: Groups = Groups::$groups;
            static DATA: [FieldDef; GROUPS.count()] = fields::data::<{ GROUPS.count() }>($kind, GROUPS);
            static TEXT: [FieldText; 2] = fields::label($kind);
            static DRESS: Dress = Dress { $($over)* ..fields::device_dress(GROUPS, $kind, &DATA, &TEXT) };
            pub(super) static MORE: More = More { dress: Some(&DRESS), hidden: false, brings: &[] };
        }
    };
}
pub(in crate::stencil) use device;

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of the rows a device of `groups` holds.
    fn names<const N: usize>(groups: Groups) -> Vec<&'static str> {
        data::<N>("Kind", groups).iter().map(|r| r.row).collect()
    }

    #[test]
    fn each_groups_count_is_the_rows_it_holds_each_row_once_and_only_the_network_name_starts_filled() {
        for (groups, rows) in [
            (Groups::Equipment, names::<9>(Groups::Equipment)),
            (Groups::Networked, names::<17>(Groups::Networked)),
            (Groups::Backbone, names::<18>(Groups::Backbone)),
            (Groups::Computer, names::<22>(Groups::Computer)),
        ] {
            assert_eq!(rows.len(), groups.count(), "{groups:?}");
            let held: Vec<&str> = (0..ROWS.len()).filter(|&at| groups.holds(at)).map(|at| ROWS[at].row).collect();
            assert_eq!(rows, held, "{groups:?}: the rows it holds, in the table's order");
            let mut once = rows.clone();
            once.sort_unstable();
            once.dedup();
            assert_eq!(once.len(), rows.len(), "{groups:?}: no row twice");
        }
        // Every row starts empty but `NetworkName`, which starts as the kind the master gives.
        let filled: Vec<(&str, &str)> =
            data::<22>("Kind", Groups::Computer).iter().filter(|r| !r.value.is_empty()).map(|r| (r.row, r.value)).collect();
        assert_eq!(filled, [("NetworkName", "Kind")]);
        // A group that holds no network rows has no label of fields: the firewall's text is plain.
        assert!(!Groups::Equipment.labelled() && Groups::Networked.labelled() && Groups::Backbone.labelled() && Groups::Computer.labelled());
        assert_eq!(names::<18>(Groups::Backbone).last().copied(), Some("Department"));
        assert!(!names::<17>(Groups::Networked).contains(&"Department") && !names::<9>(Groups::Equipment).contains(&"NetworkName"));
    }
}
