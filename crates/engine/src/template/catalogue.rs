//! The templates DiaCraft ships: Basic Diagram and Basic Flowchart, Cross-Functional Flowchart and Organization Chart. Originals: the names are the two kinds the Diagram kinds inventory lists,
//! the descriptions are ours, and a template holds only a page size, an orientation, a theme of our catalogue and the built-in stencils it opens
//! (see `docs/provenance.md`). Nothing is taken from another application's templates.

use crate::draw::paper::Orientation;

/// A template, as a description. [`crate::template::build`] makes it a document and the bytes of a `.vstx`.
#[derive(Debug)]
pub struct TemplateSpec {
    /// A lowercase word with underscores: what `file.new {template}` takes.
    pub id: &'static str,
    pub name: &'static str,
    /// What a File > New view files it under.
    pub category: &'static str,
    /// A sentence or two for the description pane.
    pub description: &'static str,
    /// The page's size: the id of a size in `page.presets`.
    pub size: &'static str,
    pub orientation: Orientation,
    /// The id of a theme of the catalogue and its variant (0 to 3); `None` is a page with no theme.
    pub theme: Option<(&'static str, u8)>,
    /// The built-in stencils the template opens, by id, in the order the Shapes window lists them; the first is the one it shows.
    pub stencils: &'static [&'static str],
    /// What the template's page holds when a drawing is started from it (none for an empty page).
    pub seed: Option<Seed>,
}

/// What a template puts on its page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seed {
    /// A cross-functional flowchart: horizontal lanes and phases, as the lanes' commands make them (`swimlane.create`).
    Swimlanes { lanes: usize, phases: usize },
    /// An org chart's first position: an Executive, at the top of the page in the middle (`orgchart.executive`), so that the drawing has a chart, the Org Chart tab, and a manager to drop positions on.
    OrgChart,
}

static TEMPLATES: [TemplateSpec; 4] = [
    TemplateSpec {
        id: "basic_diagram",
        name: "Basic Diagram",
        category: "General",
        description: "A plain landscape page to draw on with the basic shapes and the arrows. It has no theme and no scale, so shapes keep the plain look until you choose a theme.",
        size: "letter",
        orientation: Orientation::Landscape,
        theme: None,
        // The grade of the inventory row: "a blank page with basic geometric shapes and arrows" (Arrow Shapes opens beside Basic Shapes).
        stencils: &["basic", "arrows"],
        seed: None,
    },
    TemplateSpec {
        id: "basic_flowchart",
        name: "Basic Flowchart",
        category: "Flowchart",
        description: "A portrait page for a process or a decision flow, with the flowchart shapes open beside the basic shapes, and the quiet green theme Fernwood.",
        size: "letter",
        orientation: Orientation::Portrait,
        theme: Some(("fernwood", 0)),
        stencils: &["flowchart", "basic"],
        seed: None,
    },
    TemplateSpec {
        id: "cross_functional_flowchart",
        name: "Cross-Functional Flowchart",
        category: "Flowchart",
        description: "A landscape page that opens with three swimlanes and three phases already in place, to show who does which step. Drop the flowchart shapes into a lane and they move with it; the Cross-Functional Flowchart tab adds lanes and separators, turns the lanes and rotates their labels.",
        size: "letter",
        orientation: Orientation::Landscape,
        theme: None,
        // The grade of the inventory row: the Cross-Functional Flowchart stencil (the Swimlane and the Separator) beside the Basic Flowchart one.
        stencils: &["cross-functional", "flowchart", "basic"],
        seed: Some(Seed::Swimlanes { lanes: 3, phases: 3 }),
    },
    TemplateSpec {
        id: "organization_chart",
        name: "Organization Chart",
        category: "Business",
        description: "A landscape page for who reports to whom, with the Organization Chart Shapes open beside the basic shapes and an Executive already at the top. Drop a position on its manager and the two are connected and the chart is laid out again; the Org Chart tab lays the whole chart out, arranges one manager's reports and puts a picture on a position.",
        size: "letter",
        orientation: Orientation::Landscape,
        theme: None,
        // The grade of the inventory row: "the template opens a stencil of position shapes" and adds the Org Chart tab.
        stencils: &["orgchart", "basic"],
        seed: Some(Seed::OrgChart),
    },
];

/// The templates, in the order File > New lists them.
pub fn templates() -> &'static [TemplateSpec] {
    &TEMPLATES
}

/// The template a person or a program means by `key`: its id, in any case, with spaces and hyphens as underscores, or its name.
pub fn find(key: &str) -> Option<&'static TemplateSpec> {
    // No key is longer than a name: a long text is not looked at.
    if key.len() > 64 {
        return None;
    }
    let simple = |text: &str| text.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let wanted = simple(key);
    TEMPLATES.iter().find(|t| t.id == wanted || simple(t.name) == wanted)
}

/// Whether `key` is written as the path of a file and not as the name of a template: it holds a `/`, a `\` or a `.`, which no template's id or name does.
/// **The one rule** for what `file.new {template}` reads as a file (a word that is neither a template nor a path is told it is no template) and for what
/// the window reads on a thread (a word that is a typo of a template is answered at once, as a refusal in words, and not taken for a file).
pub fn is_path(key: &str) -> bool {
    key.contains(['/', '\\', '.'])
}
