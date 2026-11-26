//! What a sheet window needs to know about the sections of a sheet, as data and small functions: which display group a cell directly under a
//! sheet belongs to, the order the sections are shown in, what each section allows (rows, insert, delete), the number a reference gives a row
//! and the spelling of a section in a reference.
//!
//! [`DISPLAY_ORDER`] holds the sections of the index: the same sections, each with the index's element and kind,
//! and each lists the index's cells, named as the catalogue names them (the `N` of a file, else the label; a tab stop's number left off). A test
//! reads the index and fails when the table and the index disagree (`catalogue_spec.rs`), as it does for the catalogue itself.
//!
//! **The order of the sections is the window's, not the index's** (settled by the window's spec). It is one fixed order for
//! every kind of sheet, because the orders the spec publishes agree wherever they overlap, so a single total order satisfies all of them and no
//! per-kind order is needed:
//!
//! - the heads of the three sheets that have their own groups come first: Document Properties, Style Properties (a project rule: first)
//!   and the page's Page Properties, Page Layout and Ruler & Grid. A sheet holds the head of its own kind only, so these never meet;
//! - then a shape's order from R2: 1-D Endpoints above Shape Transform, then the sections of rows User-defined Cells, Shape Data, Hyperlinks,
//!   Connection Points, Actions, Controls, each Geometry section and Scratch, then Protection, Miscellaneous, Group Properties, Line Format, Fill
//!   Format, Text Fields, Character, Paragraph, Tabs, Annotation, Text Block Format, Text Transform, Layer Membership, Events, Foreign Image Info,
//!   Image Properties, Glue Info, Alignment, Shape Layout and Action Tag (the 2010 window's Smart tags);
//! - a page's Layers and Print Properties follow Scratch (the sections of rows, then Layers, then Print Properties, which comes before Theme
//!   Properties); the sections of rows keep the order on a page too (a project rule where no image shows both);
//! - the sections added in 2013 close the shape's list: R2 places Bevel Properties, 3-D Rotation Properties, the two gradients' stops, Theme
//!   Properties and Quick Style, and R3 (a project rule where no image places them) puts Gradient Properties and Additional Effect Properties
//!   before them and Change Shape Behavior after them;
//! - last, a section a file defines itself (MasterSelectors, and by name any other, which a window lists after this table) and Reviewer, which is
//!   the document's and comes last.
//!
//! **The cells of a section follow R12 where it names them** (how a window lays the section out: Shape Transform's Width, Height and Angle first,
//! Geometry's five own cells above its columns X, Y, A to E, the column titles of the sections of rows) and the index's order for the rest.
//!
//! **Design calls.** What a section allows (what may be added and deleted) is a design call of the sheet window where the window's
//! spec does not say (R14 settles what a shape inserts), so a reader of the window must not take every rule for the reference application's. The
//! rules live here and nowhere else, so an answer is an edit of this table.
//!
//! **Never a panic.** Every function answers `None` or an empty rule for a name it does not know, and the number functions use checked
//! arithmetic.

use std::sync::OnceLock;

use crate::sheet_ref::SheetRef;

/// What a section of the index is to a sheet: a display group, whose cells sit directly under the sheet, or a section of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// A display group: one line a cell, the cells directly under the sheet (the index's rows `one`).
    Cells,
    /// A section of rows: a table, one line a row, in a `Section` element of a file.
    Rows,
}

/// The kinds of sheet a section can be inserted on and deleted from. A master's page sheet holds what a page's does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SheetKind {
    /// A shape: a group, a guide, a foreign object, a master's shape.
    Shape,
    /// A page, or a master's page sheet.
    Page,
    /// The document.
    Document,
    /// A style.
    Style,
}

impl SheetKind {
    /// The kind of the sheet `sheet` names.
    pub fn of(sheet: &SheetRef) -> SheetKind {
        match sheet {
            SheetRef::Shape(_) => SheetKind::Shape,
            SheetRef::Page(_) | SheetRef::Master(_) => SheetKind::Page,
            SheetRef::Document => SheetKind::Document,
            SheetRef::Style(_) => SheetKind::Style,
        }
    }
}

/// What a section allows. The design gives one column for the rows added and deleted and one for the section inserted and deleted,
/// so [`SectionRules::insertable`] and [`SectionRules::deletable`] are equal in every section today; they are two fields so that a ruling can
/// split them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionRules {
    /// Rows can be added to the section, and deleted from it. `false` where the text makes the rows, or the section is one row of cells.
    pub rows_addable: bool,
    /// The section can be inserted, on the kinds of sheet in `kinds`.
    pub insertable: bool,
    /// The section can be deleted, on the kinds of sheet in `kinds`. `false` for Shape Transform: a shape always has one.
    pub deletable: bool,
    /// The kinds of sheet the section can be inserted on and deleted from; empty where it cannot be.
    pub kinds: &'static [SheetKind],
    /// Rows past this many are ignored by whatever reads the section (the index says so of the two gradients' tenth stop). A row can still be
    /// added past it, and the answer says so.
    pub row_limit: Option<u32>,
}

impl SectionRules {
    /// Whether the section can be inserted on a sheet of `kind`.
    pub fn can_insert_on(&self, kind: SheetKind) -> bool {
        self.insertable && self.kinds.contains(&kind)
    }

    /// Whether the section can be deleted from a sheet of `kind`.
    pub fn can_delete_on(&self, kind: SheetKind) -> bool {
        self.deletable && self.kinds.contains(&kind)
    }
}

/// One section of the index's Sections table.
#[derive(Debug)]
pub struct DisplaySection {
    /// The index's name for the section (`Shape Transform`, `Shape Data`): what a window titles it.
    pub label: &'static str,
    /// The `N` of the `Section` element a file holds its rows in (`Property` for Shape Data, `Annotation` for the indexed rows of the comments kept for
    /// `.vsd`, the index's D17); `None` for a display group.
    pub element: Option<&'static str>,
    /// A display group or a section of rows.
    pub kind: SectionKind,
    /// What may be added to it and deleted from it.
    pub rules: SectionRules,
    /// Its cells in the window's order (the cells the window lays out first, in its order) and the index's for the rest: for a display group
    /// the cells directly under the sheet, for a section of rows the cells a row holds (and a Geometry section's own five, which come first).
    pub cells: &'static [&'static str],
}

/// The most rows of a gradient the application reads (the index: "rows past the tenth are ignored").
const MAX_GRADIENT_STOPS: u32 = 10;

/// Every kind of sheet.
const EVERY_SHEET: &[SheetKind] = &[SheetKind::Shape, SheetKind::Page, SheetKind::Document, SheetKind::Style];

/// Nothing is added or deleted: every display group but Text Transform and Layer Membership, a section the text or another command makes, and a section no spec lists.
const NO_CHANGES: SectionRules = SectionRules { rows_addable: false, insertable: false, deletable: false, kinds: &[], row_limit: None };

/// Rows added and deleted, the section inserted and deleted on a shape only: Geometry, Connection Points and Controls.
const SHAPE_ROWS: SectionRules = SectionRules { rows_addable: true, insertable: true, deletable: true, kinds: &[SheetKind::Shape], row_limit: None };

/// Rows added and deleted, the section inserted and deleted on any sheet: Scratch, Shape Data, User-defined Cells, Hyperlinks, Actions and
/// Action Tag.
const SHEET_ROWS: SectionRules = SectionRules { rows_addable: true, insertable: true, deletable: true, kinds: EVERY_SHEET, row_limit: None };

/// The stops of a gradient: rows added and deleted, but the section belongs to the fill or the line and is not inserted or deleted.
const STOPS: SectionRules = SectionRules { rows_addable: true, insertable: false, deletable: false, kinds: &[], row_limit: Some(MAX_GRADIENT_STOPS) };

/// Text Transform and Layer Membership: one row of cells, so no rows to add, but the section is inserted and deleted whole on a shape.
const SHAPE_BLOCK: SectionRules =
    SectionRules { rows_addable: false, insertable: true, deletable: true, kinds: &[SheetKind::Shape], row_limit: None };

/// The index's sections in the window's order (of the window's spec, one order for every kind of sheet; see the module
/// documentation), with what each section allows and its cells in the window's order and the index's after.
pub static DISPLAY_ORDER: &[DisplaySection] = &[
    DisplaySection {
        label: "Document Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "AddMarkup",
            "DocLangID",
            "DocLockDuplicatePage",
            "DocLockReplace",
            "LockPreview",
            "NoCoauth",
            "OutputFormat",
            "PreviewQuality",
            "PreviewScope",
            "ViewMarkup",
        ],
    },
    DisplaySection {
        label: "Style Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["EnableFillProps", "EnableLineProps", "EnableTextProps", "HideForApply"],
    },
    DisplaySection {
        label: "Page Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "DrawingResizeType",
            "DrawingScale",
            "DrawingScaleType",
            "DrawingSizeType",
            "InhibitSnap",
            "PageHeight",
            "PageLockDuplicate",
            "PageLockReplace",
            "PageScale",
            "PageWidth",
            "ShdwObliqueAngle",
            "ShdwOffsetX",
            "ShdwOffsetY",
            "ShdwScaleFactor",
            "ShdwType",
            "UIVisibility",
        ],
    },
    DisplaySection {
        label: "Page Layout",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "AvenueSizeX",
            "AvenueSizeY",
            "AvoidPageBreaks",
            "BlockSizeX",
            "BlockSizeY",
            "CtrlAsInput",
            "DynamicsOff",
            "EnableGrid",
            "LineAdjustFrom",
            "LineAdjustTo",
            "LineJumpCode",
            "LineJumpFactorX",
            "LineJumpFactorY",
            "LineJumpStyle",
            "LineRouteExt",
            "LineToLineX",
            "LineToLineY",
            "LineToNodeX",
            "LineToNodeY",
            "PageLineJumpDirX",
            "PageLineJumpDirY",
            "PageShapeSplit",
            "PlaceDepth",
            "PlaceFlip",
            "PlaceStyle",
            "PlowCode",
            "ResizePage",
            "RouteStyle",
        ],
    },
    DisplaySection {
        label: "Ruler & Grid",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "XGridDensity",
            "XGridOrigin",
            "XGridSpacing",
            "XRulerDensity",
            "XRulerOrigin",
            "YGridDensity",
            "YGridOrigin",
            "YGridSpacing",
            "YRulerDensity",
            "YRulerOrigin",
        ],
    },
    DisplaySection {
        label: "1-D Endpoints",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["BeginX", "BeginY", "EndX", "EndY"],
    },
    DisplaySection {
        label: "Shape Transform",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["Width", "Height", "Angle", "PinX", "PinY", "LocPinX", "LocPinY", "FlipX", "FlipY", "ResizeMode"],
    },
    DisplaySection { label: "User-defined Cells", element: Some("User"), kind: SectionKind::Rows, rules: SHEET_ROWS, cells: &["Value", "Prompt"] },
    DisplaySection {
        label: "Shape Data",
        element: Some("Property"),
        kind: SectionKind::Rows,
        rules: SHEET_ROWS,
        cells: &["Label", "Prompt", "Type", "Format", "Value", "SortKey", "Invisible", "Verify", "LangID", "Calendar", "DataLinked"],
    },
    DisplaySection {
        label: "Hyperlinks",
        element: Some("Hyperlink"),
        kind: SectionKind::Rows,
        rules: SHEET_ROWS,
        cells: &["Address", "Default", "Description", "ExtraInfo", "Frame", "Invisible", "NewWindow", "SortKey", "SubAddress"],
    },
    DisplaySection {
        label: "Connection Points",
        element: Some("Connection"),
        kind: SectionKind::Rows,
        rules: SHAPE_ROWS,
        cells: &["D", "DirX", "DirY", "Type", "X", "Y", "AutoGen", "Prompt"],
    },
    DisplaySection {
        label: "Actions",
        element: Some("Actions"),
        kind: SectionKind::Rows,
        rules: SHEET_ROWS,
        cells: &[
            "Action",
            "Menu",
            "TagName",
            "ButtonFace",
            "SortKey",
            "Checked",
            "Disabled",
            "ReadOnly",
            "Invisible",
            "BeginGroup",
            "FlyoutChild",
            "Prompt",
        ],
    },
    DisplaySection {
        label: "Controls",
        element: Some("Control"),
        kind: SectionKind::Rows,
        rules: SHAPE_ROWS,
        cells: &["X", "Y", "XDyn", "YDyn", "XCon", "YCon", "CanGlue", "Prompt"],
    },
    DisplaySection {
        label: "Geometry",
        element: Some("Geometry"),
        kind: SectionKind::Rows,
        rules: SHAPE_ROWS,
        cells: &["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag", "X", "Y", "A", "B", "C", "D", "E", "Path"],
    },
    DisplaySection { label: "Scratch", element: Some("Scratch"), kind: SectionKind::Rows, rules: SHEET_ROWS, cells: &["X", "Y", "A", "B", "C", "D"] },
    DisplaySection {
        label: "Layers",
        element: Some("Layer"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &["Active", "Color", "Glue", "Lock", "Print", "Snap", "ColorTrans", "Visible", "Name", "NameUniv", "Status"],
    },
    DisplaySection {
        label: "Print Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "CenterX",
            "CenterY",
            "OnPage",
            "PageBottomMargin",
            "PageLeftMargin",
            "PageRightMargin",
            "PagesX",
            "PagesY",
            "PageTopMargin",
            "PaperKind",
            "PaperSource",
            "PrintGrid",
            "PrintPageOrientation",
            "ScaleX",
            "ScaleY",
        ],
    },
    DisplaySection {
        label: "Protection",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "LockAspect",
            "LockBegin",
            "LockCalcWH",
            "LockCrop",
            "LockCustProp",
            "LockDelete",
            "LockEnd",
            "LockFormat",
            "LockFromGroupFormat",
            "LockGroup",
            "LockHeight",
            "LockMoveX",
            "LockMoveY",
            "LockReplace",
            "LockRotate",
            "LockSelect",
            "LockTextEdit",
            "LockThemeColors",
            "LockThemeConnectors",
            "LockThemeEffects",
            "LockThemeFonts",
            "LockThemeIndex",
            "LockVariation",
            "LockVtxEdit",
            "LockWidth",
        ],
    },
    DisplaySection {
        label: "Miscellaneous",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "Calendar",
            "Comment",
            "DropOnPageScale",
            "DynFeedback",
            "HideText",
            "IsDropSource",
            "LangID",
            "LocalizeMerge",
            "NoAlignBox",
            "NoCtlHandles",
            "NoLiveDynamics",
            "NonPrinting",
            "NoObjHandles",
            "NoProofing",
            "ObjType",
            "UpdateAlignBox",
            "Copyright",
            "HelpTopic",
            "ShapeKeywords",
        ],
    },
    DisplaySection {
        label: "Group Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["DisplayMode", "DontMoveChildren", "IsDropTarget", "IsSnapTarget", "IsTextEditTarget", "SelectMode"],
    },
    DisplaySection {
        label: "Line Format",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "BeginArrow",
            "BeginArrowSize",
            "CompoundType",
            "EndArrow",
            "EndArrowSize",
            "LineCap",
            "LineColor",
            "LineColorTrans",
            "LinePattern",
            "LineWeight",
            "Rounding",
        ],
    },
    DisplaySection {
        label: "Fill Format",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "FillBkgnd",
            "FillBkgndTrans",
            "FillForegnd",
            "FillForegndTrans",
            "FillPattern",
            "ShapeShdwBlur",
            "ShapeShdwObliqueAngle",
            "ShapeShdwOffsetX",
            "ShapeShdwOffsetY",
            "ShapeShdwScaleFactor",
            "ShapeShdwShow",
            "ShapeShdwType",
            "ShdwForegnd",
            "ShdwForegndTrans",
            "ShdwPattern",
            "ShdwBkgnd",
            "ShdwBkgndTrans",
        ],
    },
    DisplaySection {
        label: "Text Fields",
        element: Some("Field"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &["Format", "Value", "Calendar", "ObjectKind", "Type", "UICat", "UICod", "UIFmt", "EditMode"],
    },
    DisplaySection {
        label: "Character",
        element: Some("Character"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &[
            "AsianFont",
            "Case",
            "Color",
            "ComplexScriptFont",
            "ComplexScriptSize",
            "DoubleStrikethrough",
            "DblUnderline",
            "Font",
            "LangID",
            "Overline",
            "Pos",
            "FontScale",
            "Size",
            "Letterspace",
            "Strikethru",
            "Style",
            "ColorTrans",
        ],
    },
    DisplaySection {
        label: "Paragraph",
        element: Some("Paragraph"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &[
            "Bullet",
            "BulletFont",
            "BulletFontSize",
            "BulletStr",
            "Flags",
            "HorzAlign",
            "IndFirst",
            "IndLeft",
            "IndRight",
            "SpAfter",
            "SpBefore",
            "SpLine",
            "TextPosAfterBullet",
            "LocalizeBulletFont",
        ],
    },
    DisplaySection { label: "Tabs", element: Some("Tabs"), kind: SectionKind::Rows, rules: NO_CHANGES, cells: &["Alignment", "Position", "X1"] },
    DisplaySection {
        label: "Annotation",
        element: Some("Annotation"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &["Comment", "Date", "LangID", "X", "Y", "ReviewerID", "MarkerIndex"],
    },
    DisplaySection {
        label: "Text Block Format",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "BottomMargin",
            "DefaultTabStop",
            "LeftMargin",
            "RightMargin",
            "TextBkgnd",
            "TextBkgndTrans",
            "TextDirection",
            "TopMargin",
            "VerticalAlign",
        ],
    },
    DisplaySection {
        label: "Text Transform",
        element: None,
        kind: SectionKind::Cells,
        rules: SHAPE_BLOCK,
        cells: &["TxtAngle", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtPinX", "TxtPinY", "TxtWidth"],
    },
    DisplaySection { label: "Layer Membership", element: None, kind: SectionKind::Cells, rules: SHAPE_BLOCK, cells: &["LayerMember"] },
    DisplaySection {
        label: "Events",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["EventDblClick", "EventDrop", "EventMultiDrop", "EventXFMod", "TheData", "TheText"],
    },
    DisplaySection {
        label: "Foreign Image Info",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["ClippingPath", "ImgHeight", "ImgOffsetX", "ImgOffsetY", "ImgWidth"],
    },
    DisplaySection {
        label: "Image Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["Blur", "Brightness", "Contrast", "Denoise", "Gamma", "Sharpen", "Transparency"],
    },
    DisplaySection {
        label: "Glue Info",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["BegTrigger", "EndTrigger", "GlueType", "WalkPreference"],
    },
    DisplaySection {
        label: "Alignment",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["AlignBottom", "AlignCenter", "AlignLeft", "AlignMiddle", "AlignRight", "AlignTop"],
    },
    DisplaySection {
        label: "Shape Layout",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "ConFixedCode",
            "ConLineJumpCode",
            "ConLineJumpDirX",
            "ConLineJumpDirY",
            "ConLineJumpStyle",
            "ConLineRouteExt",
            "DisplayLevel",
            "Relationships",
            "ShapeFixedCode",
            "ShapePermeablePlace",
            "ShapePermeableX",
            "ShapePermeableY",
            "ShapePlaceFlip",
            "ShapePlaceStyle",
            "ShapePlowCode",
            "ShapeRouteStyle",
            "ShapeSplit",
            "ShapeSplittable",
            "ZOrderChanged",
        ],
    },
    DisplaySection {
        label: "Action Tag",
        element: Some("ActionTag"),
        kind: SectionKind::Rows,
        rules: SHEET_ROWS,
        cells: &["ButtonFace", "Description", "Disabled", "DisplayMode", "TagName", "X", "XJustify", "Y", "YJustify"],
    },
    DisplaySection {
        label: "Gradient Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "FillGradientAngle",
            "FillGradientDir",
            "FillGradientEnabled",
            "LineGradientAngle",
            "LineGradientDir",
            "LineGradientEnabled",
            "RotateGradientWithShape",
            "UseGroupGradient",
        ],
    },
    DisplaySection {
        label: "Additional Effect Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "GlowColor",
            "GlowColorTrans",
            "GlowSize",
            "ReflectionBlur",
            "ReflectionDist",
            "ReflectionSize",
            "ReflectionTrans",
            "SketchAmount",
            "SketchEnabled",
            "SketchFillChange",
            "SketchLineChange",
            "SketchLineWeight",
            "SketchSeed",
            "SoftEdgesSize",
        ],
    },
    DisplaySection {
        label: "Bevel Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "BevelBottomHeight",
            "BevelBottomType",
            "BevelBottomWidth",
            "BevelContourColor",
            "BevelContourSize",
            "BevelDepthColor",
            "BevelDepthSize",
            "BevelLightingAngle",
            "BevelLightingType",
            "BevelMaterialType",
            "BevelTopHeight",
            "BevelTopType",
            "BevelTopWidth",
        ],
    },
    DisplaySection {
        label: "3-D Rotation Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["DistanceFromGround", "KeepTextFlat", "Perspective", "RotationType", "RotationXAngle", "RotationYAngle", "RotationZAngle"],
    },
    DisplaySection {
        label: "Line Gradient",
        element: Some("LineGradient"),
        kind: SectionKind::Rows,
        rules: STOPS,
        cells: &["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"],
    },
    DisplaySection {
        label: "Fill Gradient",
        element: Some("FillGradient"),
        kind: SectionKind::Rows,
        rules: STOPS,
        cells: &["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"],
    },
    DisplaySection {
        label: "Theme Properties",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "ColorSchemeIndex",
            "ConnectorSchemeIndex",
            "EffectSchemeIndex",
            "EmbellishmentIndex",
            "FontSchemeIndex",
            "ThemeIndex",
            "VariationColorIndex",
            "VariationStyleIndex",
        ],
    },
    DisplaySection {
        label: "Quick Style",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &[
            "QuickStyleEffectsMatrix",
            "QuickStyleFillColor",
            "QuickStyleFillMatrix",
            "QuickStyleFontColor",
            "QuickStyleFontMatrix",
            "QuickStyleLineColor",
            "QuickStyleLineMatrix",
            "QuickStyleShadowColor",
            "QuickStyleType",
            "QuickStyleVariation",
        ],
    },
    DisplaySection {
        label: "Change Shape Behavior",
        element: None,
        kind: SectionKind::Cells,
        rules: NO_CHANGES,
        cells: &["ReplaceCopyCells", "ReplaceLockFormat", "ReplaceLockShapeData", "ReplaceLockText"],
    },
    DisplaySection {
        label: "MasterSelectors",
        element: Some("MasterSelectors"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &["MasterColumn", "MasterMapping"],
    },
    DisplaySection {
        label: "Reviewer",
        element: Some("Reviewer"),
        kind: SectionKind::Rows,
        rules: NO_CHANGES,
        cells: &["Color", "Initials", "Name", "CurrentIndex", "ReviewerID"],
    },
];

/// The spelling a reference uses for a section, against the `N` of the file's `Section`, as `(spelling, element)`.
/// `Geometry` is spelled with its number (`Geometry2`: the section's `IX` plus one) and is in no table.
pub const SECTION_SPELLINGS: &[(&str, &str)] = &[
    ("SmartTags", "ActionTag"),
    ("Actions", "Actions"),
    ("Char", "Character"),
    ("Connections", "Connection"),
    ("Controls", "Control"),
    ("Fields", "Field"),
    ("FillGradientStops", "FillGradient"),
    ("Hyperlink", "Hyperlink"),
    ("Layers", "Layer"),
    ("LineGradientStops", "LineGradient"),
    ("Para", "Paragraph"),
    ("Prop", "Property"),
    ("Reviewer", "Reviewer"),
    ("Scratch", "Scratch"),
    ("Tabs", "Tabs"),
    ("User", "User"),
];

/// The spelling a reference uses for the section whose element is `element`: `Char` for `Character`. `None` for `Geometry` (its number follows
/// the name) and for a section with no spelling of its own, which a reference names by its own name.
pub fn section_spelling(element: &str) -> Option<&'static str> {
    SECTION_SPELLINGS.iter().find(|(_, e)| *e == element).map(|(spelling, _)| *spelling)
}

/// The element a reference's spelling stands for: `Character` for `Char`. `None` for `Geometry<n>` and for any name the table does not list.
pub fn section_for_spelling(spelling: &str) -> Option<&'static str> {
    SECTION_SPELLINGS.iter().find(|(s, _)| *s == spelling).map(|(_, element)| *element)
}

/// The section called `name`, by its label (`Shape Data`) or by the element of a file (`Property`).
pub fn display_section(name: &str) -> Option<&'static DisplaySection> {
    DISPLAY_ORDER.iter().find(|s| s.label == name).or_else(|| DISPLAY_ORDER.iter().find(|s| s.element == Some(name)))
}

/// What the section called `name` (a label or an element) allows. Nothing, for a name that is no section of the index: a section no spec lists
/// cannot be inserted, deleted or given rows.
pub fn section_rules(name: &str) -> SectionRules {
    display_section(name).map_or(NO_CHANGES, |s| s.rules)
}

/// The cells of the display groups with the group of each, sorted by cell name, built once.
fn group_index() -> &'static [(&'static str, &'static str)] {
    static INDEX: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let groups = DISPLAY_ORDER.iter().filter(|s| s.kind == SectionKind::Cells);
        let mut all: Vec<(&'static str, &'static str)> = groups.flat_map(|s| s.cells.iter().map(move |cell| (*cell, s.label))).collect();
        all.sort_unstable();
        all
    })
}

/// The display group of the cell `cell` directly under a sheet: the index's Section column (`PinX` is in `Shape Transform`, `LockWidth` in
/// `Protection`). `None` for a cell that sits in no group (the cells seen only in samples, which the index gives no section), for a name that is no
/// cell of the index, and for a cell of a row: the name `LangID` is both a cell of Miscellaneous and a cell in rows, and asked for here it is the
/// first. Names match in case.
pub fn display_group(cell: &str) -> Option<&'static str> {
    let index = group_index();
    let at = index.binary_search_by(|(name, _)| (*name).cmp(cell)).ok()?;
    index.get(at).map(|(_, group)| *group)
}

/// The `IX` of the row a reference's row number names: a Geometry row number is the `IX` itself (from samples), and the
/// number of any other indexed section is the `IX` plus one, so row 0 is no row there. `None` for row 0 outside Geometry.
pub fn row_number_to_ix(geometry: bool, n: u32) -> Option<u32> {
    if geometry { Some(n) } else { n.checked_sub(1) }
}

/// The number a reference gives the row with this `IX`: the inverse of [`row_number_to_ix`]. `None` for the largest `IX` outside Geometry, whose
/// number would not fit.
pub fn ix_to_row_number(geometry: bool, ix: u32) -> Option<u32> {
    if geometry { Some(ix) } else { ix.checked_add(1) }
}

/// The least number from 1 that is not in `taken`: the `n` of the next `Row_<n>`, `Property<n>` or `Action<n>` a new row is named, whatever the
/// order of `taken` and however often a number is repeated. A 0 in it is no row number and does not count. `None` only when every number from 1
/// to `u32::MAX` is taken.
pub fn next_row_number(taken: &[u32]) -> Option<u32> {
    let mut sorted: Vec<u32> = taken.iter().copied().filter(|n| *n >= 1).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut want = 1u32;
    for n in sorted {
        if n != want {
            // Sorted and without repeats: every later number is larger, so `want` is free.
            break;
        }
        want = want.checked_add(1)?;
    }
    Some(want)
}
