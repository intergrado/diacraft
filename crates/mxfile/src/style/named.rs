//! The named styles of the default stylesheet (S-R8): a bare token in a style string (`text`, `blue`) copies the keys of the style of that name in at
//! that place, and a token the stylesheet does not have is ignored (S-R1).
//!
//! Each style is the pairs it adds, and at most one style it extends, applied first (`edgeLabel` extends `text`; `icon` extends `label`; each of the nine colour styles extends
//! `fancy`). The pairs are written as the style strings they are, so that the one reader of values reads them, and a test holds every one of them to be a good value. The list is
//! sorted by name for binary search. A pair whose value is `none` removes its key, as in any style (S-R4): that is how `text` takes the fill away.

/// A named style: the pairs it adds, after the style it extends.
#[derive(Debug)]
pub struct Named {
    /// The token that names it.
    pub name: &'static str,
    /// The style whose pairs are applied first, if any. An extended style extends none: the depth is two at most.
    pub extends: Option<&'static str>,
    /// The pairs this style adds, in order.
    pub pairs: &'static [(&'static str, &'static str)],
}

impl Named {
    /// How many pairs applying this style applies, those of the style it extends included.
    pub fn len(&self) -> usize {
        self.pairs.len().saturating_add(self.extends.and_then(find).map_or(0, |base| base.pairs.len()))
    }

    /// Whether applying this style applies no pair.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `fillColor`, `gradientColor` and `strokeColor` of a colour style, written as a macro so that the nine colours and their nine plain twins are each said once.
macro_rules! colours {
    ($fill:literal, $gradient:literal, $stroke:literal) => {
        &[("fillColor", $fill), ("gradientColor", $gradient), ("strokeColor", $stroke)]
    };
}

/// The named styles, sorted by name (byte order: a capital letter comes before a small one).
pub const NAMED: &[Named] = &[
    Named { name: "arrow", extends: None, pairs: &[("shape", "arrow"), ("edgeStyle", "none"), ("fillColor", "default")] },
    Named { name: "blue", extends: Some("fancy"), pairs: colours!("#DAE8FC", "#7EA6E0", "#6C8EBF") },
    Named { name: "edgeLabel", extends: Some("text"), pairs: &[("labelBackgroundColor", "default"), ("fontSize", "11")] },
    Named { name: "ellipse", extends: None, pairs: &[("shape", "ellipse"), ("perimeter", "ellipsePerimeter")] },
    Named { name: "fancy", extends: None, pairs: &[("shadow", "1"), ("glass", "1")] },
    Named { name: "gray", extends: Some("fancy"), pairs: colours!("#F5F5F5", "#B3B3B3", "#666666") },
    Named { name: "green", extends: Some("fancy"), pairs: colours!("#D5E8D4", "#97D077", "#82B366") },
    Named {
        name: "group",
        extends: None,
        pairs: &[("verticalAlign", "top"), ("fillColor", "none"), ("strokeColor", "none"), ("gradientColor", "none")],
    },
    Named {
        name: "icon",
        extends: Some("label"),
        pairs: &[
            ("align", "center"),
            ("imageAlign", "center"),
            ("verticalLabelPosition", "bottom"),
            ("verticalAlign", "top"),
            ("spacingTop", "6"),
            ("spacing", "0"),
            ("spacingLeft", "0"),
            ("labelBackgroundColor", "default"),
            ("fontStyle", "0"),
            ("imageWidth", "48"),
            ("imageHeight", "48"),
        ],
    },
    Named {
        name: "image",
        extends: None,
        pairs: &[("shape", "image"), ("labelBackgroundColor", "default"), ("verticalLabelPosition", "bottom"), ("verticalAlign", "top")],
    },
    Named {
        name: "label",
        extends: None,
        pairs: &[
            ("fontStyle", "1"),
            ("align", "left"),
            ("verticalAlign", "middle"),
            ("spacing", "2"),
            ("spacingLeft", "52"),
            ("imageWidth", "42"),
            ("imageHeight", "42"),
            ("rounded", "1"),
        ],
    },
    Named {
        name: "line",
        extends: None,
        pairs: &[("shape", "line"), ("strokeWidth", "4"), ("labelBackgroundColor", "default"), ("verticalAlign", "top"), ("spacingTop", "8")],
    },
    Named { name: "orange", extends: Some("fancy"), pairs: colours!("#FFCD28", "#FFA500", "#D79B00") },
    Named { name: "pink", extends: Some("fancy"), pairs: colours!("#E6D0DE", "#B5739D", "#996185") },
    Named { name: "plain-blue", extends: None, pairs: colours!("#DAE8FC", "#7EA6E0", "#6C8EBF") },
    Named { name: "plain-gray", extends: None, pairs: colours!("#F5F5F5", "#B3B3B3", "#666666") },
    Named { name: "plain-green", extends: None, pairs: colours!("#D5E8D4", "#97D077", "#82B366") },
    Named { name: "plain-orange", extends: None, pairs: colours!("#FFCD28", "#FFA500", "#D79B00") },
    Named { name: "plain-pink", extends: None, pairs: colours!("#E6D0DE", "#B5739D", "#996185") },
    Named { name: "plain-purple", extends: None, pairs: colours!("#E1D5E7", "#8C6C9C", "#9673A6") },
    Named { name: "plain-red", extends: None, pairs: colours!("#F8CECC", "#EA6B66", "#B85450") },
    Named { name: "plain-turquoise", extends: None, pairs: colours!("#D5E8D4", "#67AB9F", "#6A9153") },
    Named { name: "plain-yellow", extends: None, pairs: colours!("#FFF2CC", "#FFD966", "#D6B656") },
    Named { name: "purple", extends: Some("fancy"), pairs: colours!("#E1D5E7", "#8C6C9C", "#9673A6") },
    Named { name: "red", extends: Some("fancy"), pairs: colours!("#F8CECC", "#EA6B66", "#B85450") },
    Named { name: "rhombus", extends: None, pairs: &[("shape", "rhombus"), ("perimeter", "rhombusPerimeter")] },
    Named { name: "rhombusImage", extends: Some("image"), pairs: &[("perimeter", "rhombusPerimeter")] },
    Named { name: "roundImage", extends: Some("image"), pairs: &[("perimeter", "ellipsePerimeter")] },
    Named { name: "swimlane", extends: None, pairs: &[("shape", "swimlane"), ("fontSize", "12"), ("fontStyle", "1"), ("startSize", "23")] },
    Named {
        name: "text",
        extends: None,
        pairs: &[("fillColor", "none"), ("gradientColor", "none"), ("strokeColor", "none"), ("align", "left"), ("verticalAlign", "top")],
    },
    Named { name: "triangle", extends: None, pairs: &[("shape", "triangle"), ("perimeter", "trianglePerimeter")] },
    Named { name: "turquoise", extends: Some("fancy"), pairs: colours!("#D5E8D4", "#67AB9F", "#6A9153") },
    Named { name: "yellow", extends: Some("fancy"), pairs: colours!("#FFF2CC", "#FFD966", "#D6B656") },
];

/// The named style a token names, or `None` for a name the stylesheet does not have.
pub fn find(name: &str) -> Option<&'static Named> {
    NAMED.binary_search_by(|named| named.name.cmp(name)).ok().and_then(|i| NAMED.get(i))
}
