//! The templates DiaCraft ships: Basic Diagram and Basic Flowchart, Cross-Functional Flowchart and Organization Chart, and the fourteen of the second stencil
//! batch (STENCIL2: UML, BPMN, C4, network, cloud, crow's foot, Chen and Gane-Sarson). Originals: the names are the kinds the Diagram kinds inventory lists,
//! the descriptions are ours, and a template holds only a page size, an orientation, a theme of our catalogue and the built-in stencils it opens
//! (see `docs/provenance.md`). Nothing is taken from another application's templates.
//!
//! **A template is registered at once and listed when it is shipped**: [`templates`] gives the templates whose stencils are all shipped (`StencilSpec::shipped`), in the order File > New lists
//! them, and [`find`] finds only those, so a template whose stencils a family task has not filled yet is in no listing and `file.new` says it is no template. [`registered`] is the whole list.

use std::sync::LazyLock;

use crate::draw::paper::Orientation;

/// A template, as a description. [`crate::template::build`] makes it a document and the bytes of a `.vstx`.
#[derive(Clone, Copy, Debug)]
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

/// The category the software notations are filed under in File > New (the word of the stencils' category).
const SOFTWARE: &str = "Software and Database";

/// A landscape Letter page with no theme, no seed and the stencils named: the second batch's templates are blank pages (STENCIL2 design, section 5).
const fn blank(
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    stencils: &'static [&'static str],
) -> TemplateSpec {
    TemplateSpec { id, name, category, description, size: "letter", orientation: Orientation::Landscape, theme: None, stencils, seed: None }
}

/// **Every template DiaCraft registers**, in the order File > New lists them. A template is listed when each stencil it names is shipped.
static TEMPLATES: [TemplateSpec; 18] = [
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
    blank(
        "uml_class",
        "UML Class",
        SOFTWARE,
        "A blank landscape page for class diagrams, based on UML 2.5.1, with the UML Class shapes open beside the basic shapes: classes, interfaces, enumerations, data types, objects, packages and notes, and the associations, aggregations, compositions, generalizations, realizations and dependencies between them. A class holds its name, attributes and operations as text that makes its compartments grow; members as separate rows are not built.",
        &["uml-class", "basic"],
    ),
    blank(
        "uml_use_case",
        "UML Use Case",
        SOFTWARE,
        "A blank landscape page for use case diagrams, based on UML 2.5.1, with the UML Use Case shapes open beside the basic shapes: actors, use cases, a subject that holds them, and the association, include, extend and generalization lines.",
        &["uml-usecase", "basic"],
    ),
    blank(
        "uml_sequence",
        "UML Sequence",
        SOFTWARE,
        "A blank landscape page for sequence diagrams, based on UML 2.5.1, with the UML Sequence shapes open beside the basic shapes: lifelines, activations, destruction, combined fragments, interaction uses and the synchronous, asynchronous, reply, create and self messages. A lifeline's foot has no handle to drag: resize the shape.",
        &["uml-sequence", "basic"],
    ),
    blank(
        "uml_activity",
        "UML Activity",
        SOFTWARE,
        "A blank landscape page for activity diagrams, based on UML 2.5.1, with the UML Activity shapes open beside the cross-functional flowchart shapes (for partitions) and the basic shapes: actions, decisions, initial and final nodes, forks and joins, object nodes, signals and the control and object flows.",
        &["uml-activity", "cross-functional", "basic"],
    ),
    blank(
        "uml_state_machine",
        "UML State Machine",
        SOFTWARE,
        "A blank landscape page for state machine diagrams, based on UML 2.5.1, with the UML State Machine shapes open beside the basic shapes: states, composite states, pseudostates, history, entry and exit points, and transitions.",
        &["uml-state", "basic"],
    ),
    blank(
        "uml_component",
        "UML Component",
        SOFTWARE,
        "A blank landscape page for component diagrams, based on UML 2.5.1, with the UML Component and Deployment shapes open beside the UML Class shapes and the basic shapes: components that hold other components, ports, provided and required interfaces, artifacts and dependencies. A required interface has no socket handle to drag.",
        &["uml-component", "uml-class", "basic"],
    ),
    blank(
        "uml_deployment",
        "UML Deployment",
        SOFTWARE,
        "A blank landscape page for deployment diagrams, based on UML 2.5.1, with the UML Component and Deployment shapes open beside the basic shapes: nodes, execution environments, devices, artifacts, and the deployment, manifestation and communication path lines.",
        &["uml-component", "basic"],
    ),
    blank(
        "bpmn_diagram",
        "BPMN Diagram",
        "Flowchart",
        "A blank landscape page for business process diagrams, based on BPMN 2.0.2, with the BPMN shapes open beside the basic shapes: tasks, sub-processes, events, gateways, data objects and stores, messages, groups, annotations, pools and lanes, and the sequence, message and data flows. A shape's variant (a task's type, an event's trigger) is chosen in its Shape Data, and the diagram is not checked against the standard's rules.",
        &["bpmn", "basic"],
    ),
    blank(
        "c4_diagram",
        "C4 Model Diagram",
        SOFTWARE,
        "A blank landscape page for software architecture diagrams, based on the C4 model: people, software systems, containers and components with their technology and description, boundaries, deployment nodes, relationships, a diagram title and a key. One template serves the context, container, component and deployment diagrams, and the C4 model's notation leaves the shapes and colours to the author, so these are DiaCraft's own.",
        &["c4", "basic"],
    ),
    blank(
        "basic_network",
        "Basic Network Diagram",
        "Network",
        "A blank landscape page for network diagrams, based on the usual network diagram symbols, with the network devices and the computers and devices open beside the basic shapes: routers, switches, firewalls, servers, desktops, laptops, printers and the links between them. Each device holds its equipment, network and computer details (its network name, IP address, location and the like) as Shape Data, and its label shows the network name over the address. A backbone with draggable arms and a legend that counts shapes are not built.",
        &["network", "computers", "basic"],
    ),
    blank(
        "cloud_architecture",
        "Cloud Architecture Diagram",
        "Network",
        "A blank landscape page for cloud architecture diagrams, based on the usual cloud architecture symbols, with generic cloud services (virtual machines, containers, functions, storage, queues, databases, gateways) open beside the network devices and the computers and devices. The services are DiaCraft's own neutral figures and carry no vendor's marks.",
        &["cloud", "network", "computers"],
    ),
    blank(
        "crows_foot",
        "Crow's Foot Database Notation",
        SOFTWARE,
        "A blank landscape page for entity-relationship diagrams, based on crow's foot notation, with the crow's foot shapes open beside the basic shapes: entities with their attributes and keys, and relationships whose line ends show the minimum and maximum of each side (exactly one, zero or one, one or many, zero or many). The cardinality of an end is chosen in the relationship's Shape Data.",
        &["crows-foot", "basic"],
    ),
    blank(
        "chen_notation",
        "Chen's Database Notation",
        SOFTWARE,
        "A blank landscape page for entity-relationship diagrams, based on Chen's notation, with the Chen shapes open beside the basic shapes: entities, weak entities and associative entities, relationships and identifying relationships, attributes (key, multivalued and derived) and the links between them, and a double line for total participation.",
        &["chen", "basic"],
    ),
    blank(
        "gane_sarson",
        "Data Flow Model (Gane-Sarson)",
        SOFTWARE,
        "A blank landscape page for data flow diagrams, based on the Gane-Sarson notation, with the Gane-Sarson shapes open beside the basic shapes: external entities, processes with rounded corners, data stores with an open side, and data flows with an arrowhead.",
        &["gane-sarson", "basic"],
    ),
];

/// Every template registered, listed or not, in the order File > New lists them. The catalogue's own tests hold the registration to its rules before a family fills a stencil.
pub fn registered() -> &'static [TemplateSpec] {
    &TEMPLATES
}

/// The templates whose stencils are all shipped, in the order File > New lists them (made once).
pub fn templates() -> &'static [TemplateSpec] {
    static LISTED: LazyLock<Vec<TemplateSpec>> = LazyLock::new(|| TEMPLATES.iter().filter(|t| listed(t)).copied().collect());
    LISTED.as_slice()
}

/// Whether a template is listed: every stencil it names is shipped.
fn listed(template: &TemplateSpec) -> bool {
    template.stencils.iter().all(|id| crate::stencil::builtin_stencil(id).is_some())
}

/// The template a person or a program means by `key`: its id, in any case, with spaces and hyphens as underscores, or its name.
pub fn find(key: &str) -> Option<&'static TemplateSpec> {
    // No key is longer than a name: a long text is not looked at.
    if key.len() > 64 {
        return None;
    }
    let simple = |text: &str| text.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let wanted = simple(key);
    templates().iter().find(|t| t.id == wanted || simple(t.name) == wanted)
}

/// Whether `key` is written as the path of a file and not as the name of a template: it holds a `/`, a `\` or a `.`, which no template's id or name does.
/// **The one rule** for what `file.new {template}` reads as a file (a word that is neither a template nor a path is told it is no template) and for what
/// the window reads on a thread (a word that is a typo of a template is answered at once, as a refusal in words, and not taken for a file).
pub fn is_path(key: &str) -> bool {
    key.contains(['/', '\\', '.'])
}
