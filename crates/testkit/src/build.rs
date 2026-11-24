//! A builder for documents: pages, masters, styles and shapes with the cells a test names, and nothing else.
//!
//! It is for the tests of every crate that reads the model (`doc`, `calc`, `vsdx`, `render`): a test says what the sheet
//! holds in a line each, and builds the document the spec's vector describes. It builds with the model's own types and
//! writes through the model's own `set_cell`, so what it makes is what a reader of a file would make.
//!
//! ```ignore
//! let doc = DocBuilder::new()
//!     .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").formula("LocPinX", "Width*0.5", "1")))
//!     .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("Width", "6")))
//!     .build();
//! ```
// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on panicking
// (AGENTS.md, Never crash) is for code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, ForeignData, Master, Page, RowKey, Shape, ShapeList, ShapeType, Sheet, Style, Text, TextPart, Value};

/// Methods every builder with a sheet has: the cells and the style links.
macro_rules! sheet_methods {
    () => {
        /// A local constant: `V` and no formula. `value` is the text exactly.
        pub fn cell(mut self, name: &str, value: &str) -> Self {
            self.put(CellAddr::direct(name), Cell::constant(Value::from_text(value)));
            self
        }

        /// A local constant with a unit.
        pub fn cell_u(mut self, name: &str, value: &str, unit: &str) -> Self {
            self.put(CellAddr::direct(name), Cell::constant(Value::from_text(value)).with_unit(unit));
            self
        }

        /// A local formula with its cached value (`F` and `V`).
        pub fn formula(mut self, name: &str, formula: &str, value: &str) -> Self {
            self.put(CellAddr::direct(name), Cell::with_formula(formula, Some(Value::from_text(value))));
            self
        }

        /// `F="Inh"`, with a cached value of its own when `value` is given.
        pub fn inh(mut self, name: &str, value: Option<&str>) -> Self {
            self.put(CellAddr::direct(name), Cell::inherited(value.map(Value::from_text)));
            self
        }

        /// `F="No Formula"` and a value.
        pub fn no_formula(mut self, name: &str, value: &str) -> Self {
            let mut cell = Cell::constant(Value::from_text(value));
            cell.formula = diacraft_doc::FormulaSlot::NoFormula;
            self.put(CellAddr::direct(name), cell);
            self
        }

        /// Any cell at any address.
        pub fn set(mut self, addr: CellAddr, cell: Cell) -> Self {
            self.put(addr, cell);
            self
        }

        /// A local constant in a row of a section: `("Geometry", Some(0), RowKey::Ix(2), "X", "1")`.
        pub fn row_cell(mut self, section: &str, section_ix: Option<u32>, row: RowKey, name: &str, value: &str) -> Self {
            self.put(CellAddr::in_row(section, section_ix, row, name), Cell::constant(Value::from_text(value)));
            self
        }

        /// A local formula and its value in a row of a section.
        pub fn row_formula(mut self, section: &str, section_ix: Option<u32>, row: RowKey, name: &str, formula: &str, value: &str) -> Self {
            self.put(CellAddr::in_row(section, section_ix, row, name), Cell::with_formula(formula, Some(Value::from_text(value))));
            self
        }

        /// `F="Inh"` in a row of a section.
        pub fn row_inh(mut self, section: &str, section_ix: Option<u32>, row: RowKey, name: &str, value: Option<&str>) -> Self {
            self.put(CellAddr::in_row(section, section_ix, row, name), Cell::inherited(value.map(Value::from_text)));
            self
        }

        /// Marks a row deleted (`Del="1"`), making it if it is not there.
        pub fn del_row(mut self, section: &str, section_ix: Option<u32>, row: RowKey) -> Self {
            let key = diacraft_doc::SectionKey::new(section, section_ix);
            self.sheet.row_mut(&key, &row).expect("a fixture row is within the limits").del = true;
            self
        }

        /// Marks a section deleted (`Del="1"`), making it if it is not there.
        pub fn del_section(mut self, section: &str, section_ix: Option<u32>) -> Self {
            let key = diacraft_doc::SectionKey::new(section, section_ix);
            self.sheet.section_mut(&key).expect("a fixture section is within the limits").del = true;
            self
        }

        /// The row type `T` of a row, making the row if it is not there.
        pub fn row_type(mut self, section: &str, section_ix: Option<u32>, row: RowKey, row_type: &str) -> Self {
            let key = diacraft_doc::SectionKey::new(section, section_ix);
            self.sheet.row_mut(&key, &row).expect("a fixture row is within the limits").row_type = Some(Arc::from(row_type));
            self
        }

        /// The three style links, by style `ID` (`None` for no link).
        pub fn links(mut self, line: Option<u32>, fill: Option<u32>, text: Option<u32>) -> Self {
            self.sheet.links = diacraft_doc::Links { line, fill, text };
            self
        }

        fn put(&mut self, addr: CellAddr, cell: Cell) {
            self.sheet.set_cell(&addr, cell).expect("a fixture cell is within the limits");
        }
    };
}

/// A shape: its `ID`, ties to a master, cells and members.
#[derive(Debug, Default)]
pub struct ShapeBuilder {
    shape: Shape,
    sheet: Sheet,
    children: Vec<ShapeBuilder>,
}

impl ShapeBuilder {
    /// A shape with this `ID`.
    pub fn new(id: u32) -> ShapeBuilder {
        ShapeBuilder { shape: Shape::new(id), ..ShapeBuilder::default() }
    }

    /// An instance of the master with this `ID`.
    pub fn master(mut self, id: u32) -> Self {
        self.shape.master = Some(id);
        self
    }

    /// A member that stands for the shape with this `ID` in its group's master.
    pub fn master_shape(mut self, id: u32) -> Self {
        self.shape.master_shape = Some(id);
        self
    }

    /// `Type="Group"`.
    pub fn group(mut self) -> Self {
        self.shape.kind = ShapeType::Group;
        self
    }

    /// `Del="1"`.
    pub fn deleted(mut self) -> Self {
        self.shape.del = true;
        self
    }

    /// `Name` and `NameU`.
    pub fn named(mut self, name: &str, name_u: &str) -> Self {
        self.shape.name = Some(Arc::from(name));
        self.shape.name_u = Some(Arc::from(name_u));
        self
    }

    /// One of the three `Data` boxes (`which` 1 to 3).
    pub fn data(mut self, which: u8, text: &str) -> Self {
        let text = Some(Arc::from(text));
        match which {
            1 => self.shape.data1 = text,
            2 => self.shape.data2 = text,
            _ => self.shape.data3 = text,
        }
        self
    }

    /// The shape's `Type`.
    pub fn kind(mut self, kind: ShapeType) -> Self {
        self.shape.kind = kind;
        self
    }

    /// A member, after the others.
    pub fn child(mut self, child: ShapeBuilder) -> Self {
        self.children.push(child);
        self
    }

    /// The shape's text: one run of characters.
    pub fn text(mut self, characters: &str) -> Self {
        let mut text = Text::default();
        text.parts.push(TextPart::Chars(Arc::from(characters)));
        self.shape.text = Some(Arc::new(text));
        self
    }

    /// A `Foreign` shape showing this `ForeignData` (a picture, an embedded object).
    pub fn foreign(mut self, data: ForeignData) -> Self {
        self.shape.kind = ShapeType::Foreign;
        self.shape.foreign = Some(Arc::new(data));
        self
    }

    /// The shape's text, piece by piece: characters and the field markers a test names.
    pub fn text_parts(mut self, parts: Vec<TextPart>) -> Self {
        self.shape.text = Some(Arc::new(Text { parts, ..Default::default() }));
        self
    }

    sheet_methods!();

    fn build(self) -> Arc<Shape> {
        let mut shape = self.shape;
        shape.sheet = Arc::new(self.sheet);
        shape.children = self.children.into_iter().map(ShapeBuilder::build).collect();
        Arc::new(shape)
    }
}

/// A page, with its page sheet and top-level shapes.
#[derive(Debug, Default)]
pub struct PageBuilder {
    sheet: Sheet,
    shapes: Vec<ShapeBuilder>,
    name: Option<String>,
    back_page: Option<u32>,
    background: bool,
}

impl PageBuilder {
    pub fn new() -> PageBuilder {
        PageBuilder::default()
    }

    /// The page's name (else "Page-<id>").
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    /// `BackPage`.
    pub fn back_page(mut self, id: u32) -> Self {
        self.back_page = Some(id);
        self
    }

    /// `Background="1"`.
    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    /// A top-level shape, after the others.
    pub fn shape(mut self, shape: ShapeBuilder) -> Self {
        self.shapes.push(shape);
        self
    }

    sheet_methods!();

    fn build(self, id: u32) -> Arc<Page> {
        let name = self.name.unwrap_or_else(|| format!("Page-{id}"));
        let mut page = Page::new(id, &name);
        page.sheet = Arc::new(self.sheet);
        page.back_page = self.back_page;
        page.background = self.background;
        page.shapes = self.shapes.into_iter().map(ShapeBuilder::build).collect::<ShapeList>();
        Arc::new(page)
    }
}

/// A master, with its page sheet and the shapes of its part.
#[derive(Debug, Default)]
pub struct MasterBuilder {
    sheet: Sheet,
    shapes: Vec<ShapeBuilder>,
    name: Option<String>,
}

impl MasterBuilder {
    pub fn new() -> MasterBuilder {
        MasterBuilder::default()
    }

    /// `Name` and `NameU`.
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    /// A top-level shape of the master part, after the others.
    pub fn shape(mut self, shape: ShapeBuilder) -> Self {
        self.shapes.push(shape);
        self
    }

    sheet_methods!();

    fn build(self, id: u32) -> Arc<Master> {
        let name: Option<Arc<str>> = self.name.as_deref().map(Arc::from);
        Arc::new(Master {
            id,
            name: name.clone(),
            name_u: name,
            sheet: Arc::new(self.sheet),
            shapes: self.shapes.into_iter().map(ShapeBuilder::build).collect::<ShapeList>(),
            ..Master::default()
        })
    }
}

/// A style sheet: its cells and its links to the styles it is based on.
#[derive(Debug, Default)]
pub struct StyleBuilder {
    sheet: Sheet,
    name: Option<String>,
}

impl StyleBuilder {
    pub fn new() -> StyleBuilder {
        StyleBuilder::default()
    }

    /// `Name` and `NameU`.
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    sheet_methods!();

    fn build(self, id: u32) -> Arc<Style> {
        let name: Option<Arc<str>> = self.name.as_deref().map(Arc::from);
        Arc::new(Style { id, name: name.clone(), name_u: name, sheet: Arc::new(self.sheet), ..Style::default() })
    }
}

/// A document, built from nothing: the pages, masters and styles a test adds, and the root style 0 with the three
/// `Enable...Props` flags at 1, which every real file holds.
#[derive(Debug)]
pub struct DocBuilder {
    root: StyleBuilder,
    styles: Vec<Arc<Style>>,
    masters: Vec<Arc<Master>>,
    pages: Vec<Arc<Page>>,
    doc_sheet: Sheet,
}

impl Default for DocBuilder {
    fn default() -> Self {
        DocBuilder::new()
    }
}

impl DocBuilder {
    /// A document with the root style and nothing else.
    pub fn new() -> DocBuilder {
        let root = StyleBuilder::new().name("No Style").cell("EnableLineProps", "1").cell("EnableFillProps", "1").cell("EnableTextProps", "1");
        DocBuilder { root, styles: Vec::new(), masters: Vec::new(), pages: Vec::new(), doc_sheet: Sheet::default() }
    }

    /// Adds to the root style, `ID` 0, which starts with the three flags: `|s| s.cell("SelectMode", "1")`.
    pub fn root(mut self, f: impl FnOnce(StyleBuilder) -> StyleBuilder) -> Self {
        self.root = f(std::mem::take(&mut self.root));
        self
    }

    /// A style with this `ID`.
    pub fn style(mut self, id: u32, style: StyleBuilder) -> Self {
        self.styles.push(style.build(id));
        self
    }

    /// A master with this `ID`.
    pub fn master(mut self, id: u32, master: MasterBuilder) -> Self {
        self.masters.push(master.build(id));
        self
    }

    /// A page with this `ID`.
    pub fn page(mut self, id: u32, page: PageBuilder) -> Self {
        self.pages.push(page.build(id));
        self
    }

    /// A cell on the `DocumentSheet`.
    pub fn doc_cell(mut self, name: &str, value: &str) -> Self {
        self.doc_sheet.set_cell(&CellAddr::direct(name), Cell::constant(Value::from_text(value))).expect("a fixture cell is within the limits");
        self
    }

    /// The document.
    pub fn build(self) -> Document {
        let mut doc = Document::empty();
        doc.styles.push(self.root.build(0));
        for style in self.styles {
            doc.styles.push(style);
        }
        doc.masters = self.masters.into_iter().collect();
        doc.pages = self.pages.into_iter().collect();
        doc.sheet = Arc::new(self.doc_sheet);
        doc
    }
}
