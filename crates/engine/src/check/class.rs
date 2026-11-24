//! The classes of difference the check after a save names.
//!
//! A class has a **stable id** (what a program reads in the save result's `checked` field), **words** for a person, and a count of the **items** that differ, in the unit the class names
//! (a shape, a paragraph, a picture, a formula). The ids and the words are the vocabulary of the check, and this table is the one place they are written.

/// A class of difference between the drawing a model writer was given and the drawing the file it wrote reads back as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    /// Pages missing, added, renamed, resized or turned into background pages.
    Pages,
    /// Shapes missing, added or of another kind.
    Shapes,
    /// Shapes that have another number (`ID`) in the file, so that a reference by number may name another shape.
    ShapeNumbers,
    /// The position, size and outline of shapes.
    Geometry,
    /// The characters of a text, paragraph by paragraph.
    Text,
    /// The formatting of a text: characters, paragraphs, tabs, the text block.
    TextFormatting,
    /// Pictures (a frame's image, a bitmap fill's bytes).
    Pictures,
    /// Embedded objects.
    Objects,
    /// The macro project.
    Macros,
    /// Font faces.
    Fonts,
    /// Line ends (markers, arrows).
    LineEnds,
    /// Gradients.
    Gradients,
    /// The rest of the look: fills, lines, shadows, hatches, dashes, themes.
    StyleContent,
    /// Style sheets: missing, renamed or based on another.
    StyleSheets,
    /// Masters, master pages and layouts, and the shapes that are instances of a master.
    Masters,
    /// The layer set and what is on each layer.
    Layers,
    /// Connections: the ends a connector is glued to, and glue points.
    Connections,
    /// Formulas that do not come back as formulas: the value may be the same.
    Formulas,
    /// Hyperlinks.
    Hyperlinks,
    /// Fields in a text.
    Fields,
    /// User cells and shape data.
    UserCells,
    /// Any other cell of a shape, page, master or style sheet whose value is not the same.
    ShapeAttributes,
    /// The document's properties: title, author, keywords, custom properties.
    Properties,
    /// Comments: their authors, text, dates, replies, Done marks and the shapes and pages they are on.
    Comments,
    /// Data kept beside the cells (attributes, elements and parts a format has no cell for) that does not come back as it was.
    KeptData,
}

/// Every class, in the order the check lists them.
pub const ALL: [Class; 25] = [
    Class::Pages,
    Class::Shapes,
    Class::ShapeNumbers,
    Class::Geometry,
    Class::Text,
    Class::TextFormatting,
    Class::Pictures,
    Class::Objects,
    Class::Macros,
    Class::Fonts,
    Class::LineEnds,
    Class::Gradients,
    Class::StyleContent,
    Class::StyleSheets,
    Class::Masters,
    Class::Layers,
    Class::Connections,
    Class::Formulas,
    Class::Hyperlinks,
    Class::Fields,
    Class::UserCells,
    Class::ShapeAttributes,
    Class::Properties,
    Class::Comments,
    Class::KeptData,
];

impl Class {
    /// The bit of the class in a set of classes.
    pub(crate) const fn bit(self) -> u32 {
        1 << (self as u8)
    }

    /// The stable id a program reads: lower case, words joined by hyphens.
    pub const fn id(self) -> &'static str {
        match self {
            Class::Pages => "pages",
            Class::Shapes => "shapes",
            Class::ShapeNumbers => "shape-numbers",
            Class::Geometry => "geometry",
            Class::Text => "text",
            Class::TextFormatting => "text-formatting",
            Class::Pictures => "pictures",
            Class::Objects => "objects",
            Class::Macros => "macro-project",
            Class::Fonts => "fonts",
            Class::LineEnds => "line-ends",
            Class::Gradients => "gradients",
            Class::StyleContent => "style-content",
            Class::StyleSheets => "style-sheets",
            Class::Masters => "masters",
            Class::Layers => "layers",
            Class::Connections => "connections",
            Class::Formulas => "formulas",
            Class::Hyperlinks => "hyperlinks",
            Class::Fields => "fields",
            Class::UserCells => "user-cells",
            Class::ShapeAttributes => "shape-attributes",
            Class::Properties => "properties",
            Class::Comments => "comments",
            Class::KeptData => "kept-data",
        }
    }

    /// The class an id names.
    pub fn of_id(id: &str) -> Option<Class> {
        ALL.into_iter().find(|class| class.id() == id)
    }

    /// The words that name the class, as the card lists it.
    pub const fn label(self) -> &'static str {
        match self {
            Class::Pages => "pages",
            Class::Shapes => "shapes",
            Class::ShapeNumbers => "shape numbers",
            Class::Geometry => "geometry",
            Class::Text => "text",
            Class::TextFormatting => "text formatting",
            Class::Pictures => "pictures",
            Class::Objects => "embedded objects",
            Class::Macros => "macro project",
            Class::Fonts => "fonts",
            Class::LineEnds => "line ends",
            Class::Gradients => "gradients",
            Class::StyleContent => "style content (fills, lines, shadows, hatches, dashes, themes)",
            Class::StyleSheets => "style sheets",
            Class::Masters => "masters, master pages and layouts",
            Class::Layers => "layers",
            Class::Connections => "connections and glue",
            Class::Formulas => "formulas",
            Class::Hyperlinks => "hyperlinks",
            Class::Fields => "fields",
            Class::UserCells => "user cells and shape data",
            Class::ShapeAttributes => "other shape attributes",
            Class::Properties => "document properties",
            Class::Comments => "comments",
            Class::KeptData => "data kept beside the cells",
        }
    }

    /// What a count of the class counts, singular and plural, and what is true of them, for one and for many.
    const fn items(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Class::Pages => ("page", "pages", "differs", "differ"),
            Class::Shapes => ("shape", "shapes", "is missing, added or of another kind", "are missing, added or of another kind"),
            Class::ShapeNumbers => ("shape", "shapes", "has another number", "have another number"),
            Class::Geometry => ("shape", "shapes", "differs in position, size or outline", "differ in position, size or outline"),
            Class::Text => ("paragraph", "paragraphs", "differs", "differ"),
            Class::TextFormatting => ("shape", "shapes", "differs in the formatting of its text", "differ in the formatting of their text"),
            Class::Pictures => ("picture", "pictures", "is missing or differs", "are missing or differ"),
            Class::Objects => ("object", "objects", "is missing or differs", "are missing or differ"),
            Class::Macros => ("part", "parts", "is missing or differs", "are missing or differ"),
            Class::Fonts => ("font", "fonts", "is missing", "are missing"),
            Class::LineEnds => ("line end", "line ends", "differs", "differ"),
            Class::Gradients => ("gradient", "gradients", "differs", "differ"),
            Class::StyleContent => ("item", "items", "differs", "differ"),
            Class::StyleSheets => ("style sheet", "style sheets", "is missing or differs", "are missing or differ"),
            Class::Masters => ("item", "items", "differs", "differ"),
            Class::Layers => ("layer", "layers", "differs", "differ"),
            Class::Connections => ("connection", "connections", "differs", "differ"),
            Class::Formulas => ("formula", "formulas", "is lost", "are lost"),
            Class::Hyperlinks => ("hyperlink", "hyperlinks", "differs", "differ"),
            Class::Fields => ("field", "fields", "differs", "differ"),
            Class::UserCells => ("row", "rows", "differs", "differ"),
            Class::ShapeAttributes => ("shape", "shapes", "differs", "differ"),
            Class::Properties => ("property", "properties", "differs", "differ"),
            Class::Comments => ("comment", "comments", "is missing or differs", "are missing or differ"),
            Class::KeptData => ("item", "items", "differs", "differ"),
        }
    }

    /// The class in words with its count: `3 pictures are missing or differ`.
    pub fn words(self, count: u64) -> String {
        let (one, many, is, are) = self.items();
        if count == 1 { format!("1 {one} {is}") } else { format!("{count} {many} {are}") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_has_a_bit_an_id_that_is_its_own_and_words() {
        let mut seen = 0u32;
        let mut ids = std::collections::HashSet::new();
        for class in ALL {
            assert_eq!(seen & class.bit(), 0, "{class:?} has a bit of its own");
            seen |= class.bit();
            assert!(ids.insert(class.id()), "{class:?}: the id is its own");
            assert_eq!(Class::of_id(class.id()), Some(class));
            assert!(class.id().bytes().all(|b| b.is_ascii_lowercase() || b == b'-'), "{}", class.id());
            assert!(!class.label().is_empty() && class.words(2).starts_with("2 "));
        }
        assert_eq!(ALL.len(), 25);
        assert_eq!(Class::of_id("no such class"), None);
    }

    #[test]
    fn the_words_follow_the_number() {
        assert_eq!(Class::Formulas.words(1), "1 formula is lost");
        assert_eq!(Class::Formulas.words(5), "5 formulas are lost");
        assert_eq!(Class::Pictures.words(1), "1 picture is missing or differs");
        assert_eq!(Class::Pages.words(1), "1 page differs");
        assert_eq!(Class::ShapeNumbers.words(1), "1 shape has another number");
        assert_eq!(Class::ShapeNumbers.words(2), "2 shapes have another number");
    }
}
