//! The things a document holds besides its own sheet: pages, masters, styles and themes.

use std::sync::Arc;

use diacraft_color::{ColorScheme, Rgba};

use crate::odf::{OdfPage, OdfStyle};
use crate::package::{Ext, Kept};
use crate::raw::{Passthrough, RawAttrs};
use crate::shape::ShapeList;
use crate::sheet::Sheet;
use crate::theme_data::ThemeEntries;

/// A `Connect` element: one connection between two shapes. MS-VSDX calls them unused; the sample files
/// routing oracle reads them, and DiaCraft keeps them in step with what a connector's cells say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connect {
    pub from_sheet: u32,
    pub from_cell: Option<Arc<str>>,
    pub from_part: Option<u32>,
    pub to_sheet: u32,
    pub to_cell: Option<Arc<str>>,
    pub to_part: Option<u32>,
    pub raw: RawAttrs,
    /// What the element holds that no field reads, tagged by format; see [`crate::package`].
    pub ext: Ext<Kept>,
}

/// The `Connect` elements of a page or master part. A shared list: a page's `Connects` are copied by an edit only if the edit is to them,
/// so a snapshot of a page with a million of them (100 KB deflated) costs one pointer, not 62 MB (the review of the first round).
/// Cheap to clone, and a null pointer when there are none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connects(Option<Arc<Vec<Connect>>>);

impl Connects {
    /// No connects.
    pub fn new() -> Connects {
        Connects(None)
    }

    /// These connects, in this order.
    pub fn from_vec(connects: Vec<Connect>) -> Connects {
        if connects.is_empty() { Connects(None) } else { Connects(Some(Arc::new(connects))) }
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |list| list.len())
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The connect at `at`.
    pub fn get(&self, at: usize) -> Option<&Connect> {
        self.0.as_ref().and_then(|list| list.get(at))
    }

    /// The connects in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Connect> {
        self.0.as_ref().map_or_else(|| [].iter(), |list| list.iter())
    }

    /// The list behind the pointer it is shared by: a snapshot of the page holds the same one, and a count of the memory a model holds counts it once. `None` when there are none.
    pub fn shared_list(&self) -> Option<&Arc<Vec<Connect>>> {
        self.0.as_ref()
    }

    /// Adds a connect after the others. Copies the list first if a snapshot shares it.
    pub fn push(&mut self, connect: Connect) {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new()))).push(connect);
    }

    /// Whether this is the very list `other` is: nothing was copied between them.
    pub fn shares_with(&self, other: &Connects) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl FromIterator<Connect> for Connects {
    fn from_iter<I: IntoIterator<Item = Connect>>(iter: I) -> Connects {
        Connects::from_vec(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a Connects {
    type Item = &'a Connect;
    type IntoIter = std::slice::Iter<'a, Connect>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// One page of a document: a row of the pages index with its sheet and, when it has a part, its shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// `ID`: an opaque unsigned 32-bit key; 0 is usual in files. Unique within the document.
    pub id: u32,
    /// `Name`: what a person typed, and what the tabs show.
    pub name: Arc<str>,
    /// `NameU`: the universal name. Opening a file reads it; renaming a page leaves it, and a new page gets the name.
    pub name_u: Option<Arc<str>>,
    /// `Background`: this page is a background page. Absent means foreground.
    pub background: bool,
    /// `BackPage`: the `ID` of this page's background page. A hostile file can make these a cycle: see
    /// [`crate::Document::background_chain`].
    pub back_page: Option<u32>,
    /// The `r:id` of the `Rel` naming the page's part, kept so a writer gives the relationship back under the same id. `None`
    /// for a page with no part: such a page has a sheet and no shapes.
    pub rel_id: Option<Arc<str>>,
    /// The `PageSheet`: `PageWidth`, `PageHeight` and the rest.
    pub sheet: Arc<Sheet>,
    /// The shapes of the page part, in file order.
    pub shapes: ShapeList,
    pub connects: Connects,
    /// `ViewScale`, `ViewCenterX`, `ViewCenterY`, `IsCustomName`, `ReviewerID`, `AssociatedPage`, and any other attribute.
    pub raw: RawAttrs,
    pub passthrough: Passthrough,
    /// The page's ODF data (master page and layout names and properties, units, the stamp of its cells) and the VSDX elements and unknown `.diacraft`
    /// content it had no slot for. Filled by the `.odg` reader, the VSDX reader.
    pub ext: Ext<OdfPage>,
    /// The name of the page's part in the package it came from, beside [`Page::rel_id`]: a writer gives the part back under it, so relationships from
    /// held parts point at the right part. `None` for a page that came from no package. Filled by the VSDX reader.
    pub part_name: Option<Arc<str>>,
}

impl Page {
    /// A page with this `ID` and name, an empty sheet and no shapes.
    pub fn new(id: u32, name: &str) -> Page {
        Page {
            id,
            name: Arc::from(name),
            name_u: Some(Arc::from(name)),
            background: false,
            back_page: None,
            rel_id: None,
            sheet: Arc::new(Sheet::default()),
            shapes: ShapeList::new(),
            connects: Connects::default(),
            raw: RawAttrs::new(),
            passthrough: Passthrough::new(),
            ext: Ext::default(),
            part_name: None,
        }
    }

    /// The page as the page of a copy: it names no part and no relationship of the package it was read from (`part_name`, `rel_id`, what the `Rel` of its row held, and what the
    /// root of its part held: the spelling of the package it came from), and the package it is written into gives it a part of its own, so that two pages never name one part.
    /// What is the page's own stays, as for [`Master::leave_package`]: the nodes its `Shapes` and its `Connects` hold beside the shapes, its ODF and unknown `.diacraft`
    /// content, and the prefixes the root of its part declared (an attribute or an element kept below the root may be in one of them).
    pub fn leave_package(&mut self) {
        self.rel_id = None;
        self.part_name = None;
        if !self.ext.is_empty() {
            let rest = &mut self.ext.make_mut().vsdx.0;
            rest.retain(|(key, _)| &**key != "Rel");
            for (key, kept) in rest.iter_mut() {
                if &**key == "PageContents" {
                    let declared = RawAttrs::from_pairs(kept.raw.iter().filter(|(name, _)| name.starts_with("xmlns:")));
                    *kept = Kept { raw: declared, kept: Passthrough::new() };
                }
            }
            rest.retain(|(key, kept)| &**key != "PageContents" || !kept.raw.is_empty());
            self.ext.tidy();
        }
    }
}

/// A master: a row of the masters index with its page sheet and the shapes of its part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Master {
    /// `ID`: 0 is allowed (R19 of the same spec). Unique within the document.
    pub id: u32,
    pub name: Option<Arc<str>>,
    pub name_u: Option<Arc<str>>,
    /// `UniqueID`: the GUID naming the master in this drawing.
    pub unique_id: Option<Arc<str>>,
    /// `PatternFlags`: 1 and 65 line patterns, 2 and 1026 line ends, 4100 and 20484 fill patterns; other numbers are kept.
    pub pattern_flags: Option<u32>,
    /// The `r:id` of the `Rel` naming the master's part, kept for the writer. `None` for a master with no part.
    pub rel_id: Option<Arc<str>>,
    /// `Icon`: base64 text, verbatim, unused by a viewer.
    pub icon: Option<Arc<str>>,
    /// The master's `PageSheet`.
    pub sheet: Arc<Sheet>,
    /// The shapes of the master part. Usually one top-level shape; a master with two or more is passed to an instance as
    /// members. Their `Master` and `MasterShape` are not read: a master step is one level.
    pub shapes: ShapeList,
    pub connects: Connects,
    /// `BaseID`, `MatchByName`, `IsCustomName`, `IsCustomNameU`, `IconSize`, `Prompt`, `Hidden`, `IconUpdate`, `AlignName`,
    /// `MasterType`, and any other attribute.
    pub raw: RawAttrs,
    pub passthrough: Passthrough,
    /// The VSDX elements and unknown `.diacraft` content the master had no slot for (its ODF side is empty). Filled by the VSDX reader.
    pub ext: Ext<Kept>,
    /// The name of the master's part, beside [`Master::rel_id`]: see [`Page::part_name`].
    pub part_name: Option<Arc<str>>,
}

impl Master {
    /// The master as a part of another drawing's package: it names no part and no relationship of the package it was read from (`part_name`, `rel_id`, what the `Rel` of its row held, and
    /// what the root of its part held: the spelling of the package it came from), and the package of the drawing it goes to gives it its own, so that two masters never name one part. What is
    /// the master's own stays: the nodes its `Shapes` and its `Connects` hold beside the shapes and the rows (unknown content survives, AGENTS.md; an `.odg` carries them in its document file,
    /// so a second save of the drawing it goes to is no edit), whether its row had a `PageSheet`, and **the prefixes the root of its part declared** (`xmlns:` and nothing else
    /// of the root): an attribute or an element kept below the root may be in one of them, and a part written without its declaration would not be XML a reader of namespaces takes.
    pub fn leave_package(&mut self) {
        self.rel_id = None;
        self.part_name = None;
        if !self.ext.is_empty() {
            let rest = &mut self.ext.make_mut().vsdx.0;
            rest.retain(|(key, _)| &**key != "Rel");
            for (key, kept) in rest.iter_mut() {
                if &**key == "MasterContents" {
                    let declared = RawAttrs::from_pairs(kept.raw.iter().filter(|(name, _)| name.starts_with("xmlns:")));
                    *kept = Kept { raw: declared, kept: Passthrough::new() };
                }
            }
            rest.retain(|(key, kept)| &**key != "MasterContents" || !kept.raw.is_empty());
            self.ext.tidy();
        }
    }
}

/// A style sheet: cells and sections like any sheet, and three links to the styles it is based on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    /// `ID`, unique in the list. Style 0, `No Style`, is the root every style reaches.
    pub id: u32,
    pub name: Option<Arc<str>>,
    pub name_u: Option<Arc<str>>,
    /// The style's cells, sections and its three links to other styles.
    pub sheet: Arc<Sheet>,
    /// `IsCustomName`, `IsCustomNameU`, and any other attribute.
    pub raw: RawAttrs,
    pub passthrough: Passthrough,
    /// The style's ODF data (name, display name, parent, family, unmapped properties, units) and the VSDX elements and unknown `.diacraft` content it
    /// had no slot for. Filled by the `.odg` reader, the VSDX reader.
    pub ext: Ext<OdfStyle>,
}

/// A DrawingML theme part: the bytes, which are kept as they are unless a command edits them, and what the
/// reader took from them for the colours and fonts a viewer needs. The theme's selections and effects are in the `Theme`
/// style's cells and in the bytes. `entries` is what the theme functions read of the rest: the variants, the style lists and the reference app's
/// extensions.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Theme {
    /// The part's name in the package.
    pub part: Arc<str>,
    /// The part's bytes, verbatim.
    pub bytes: Arc<[u8]>,
    /// `a:clrScheme`'s twelve colours, if the part has a colour scheme **and all twelve could be read**: no colour is invented
    /// So one slot that cannot be read leaves this `None` and the others are in `slots`.
    pub colors: Option<ColorScheme>,
    /// The twelve slots of `a:clrScheme` one by one, in the order `dk1`, `lt1`, `dk2`, `lt2`, `accent1` to `accent6`, `hlink`, `folHlink`: a
    /// slot is `None` when the part has no such colour or it is not one the reader reads (an `a:srgbClr`, or an `a:sysClr` with its
    /// `lastClr` or one of the two system colours). MS-VSDX's viewer ignores `dk2`, `lt2`, `hlink` and `folHlink`, so a part that spoils
    /// one of them still gives the other eleven.
    pub slots: [Option<Rgba>; 12],
    /// `a:majorFont` and `a:minorFont` Latin typefaces.
    pub major_font: Option<Arc<str>>,
    pub minor_font: Option<Arc<str>>,
    /// The `vt:schemeID` of each of the theme's five components: the number a `ColorSchemeIndex`,
    /// `FontSchemeIndex`, `EffectSchemeIndex`, `ConnectorSchemeIndex` or `ThemeIndex` cell selects it by, found by the child element's name
    /// and not by the extension's URI. `None` where the part has no such extension.
    pub color_scheme_id: Option<SchemeId>,
    pub font_scheme_id: Option<SchemeId>,
    pub effect_scheme_id: Option<SchemeId>,
    pub connector_scheme_id: Option<SchemeId>,
    pub theme_scheme_id: Option<SchemeId>,
    /// The variants and style lists the theme functions read (see [`ThemeEntries`]); empty where the part has none or none could be read.
    pub entries: ThemeEntries,
}

impl Theme {
    /// The colour a scheme slot holds in this part: `dk1` and `tx1` the first of `slots`, and so on. `None` for `phClr` (which stands for
    /// another colour), and for a slot the reader could not read.
    pub fn slot_colour(&self, slot: diacraft_color::SchemeSlot) -> Option<Rgba> {
        use diacraft_color::SchemeSlot as S;
        let at = match slot {
            S::Dk1 | S::Tx1 => 0,
            S::Lt1 | S::Bg1 => 1,
            S::Dk2 | S::Tx2 => 2,
            S::Lt2 | S::Bg2 => 3,
            S::Accent1 => 4,
            S::Accent2 => 5,
            S::Accent3 => 6,
            S::Accent4 => 7,
            S::Accent5 => 8,
            S::Accent6 => 9,
            S::Hlink => 10,
            S::FolHlink => 11,
            S::PhClr => return None,
        };
        self.slots.get(at).copied().flatten()
    }
}

/// A `vt:schemeID`: `schemeEnum` is the index a cell selects the component by; 65535 means the colour scheme comes from
/// the master whose `UniqueID` is `schemeGUID`, and for every other number `schemeGUID` is ignored (it is kept as the file had it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeId {
    pub scheme_enum: u32,
    pub scheme_guid: Option<Arc<str>>,
}
