//! The templates and the list of stencils they open: the catalogue is consistent, each template is made into a package that reads back as
//! itself with nothing odd, and the list a template gives is read the same way whatever it holds.

use diacraft_doc::{Document, Reading};
use diacraft_vsdx::{Kind, Limits, Variant, read};

use super::names::{MAX_NAMES, parse};
use super::*;
use crate::draw::paper::{Orientation, size_preset};
use crate::stencil;
use crate::theme;

fn build_one(spec: &TemplateSpec) -> Built {
    build("test", spec).unwrap_or_else(|e| panic!("{}: {e}", spec.id))
}

fn number(doc: &Document, page: u32, cell: &str) -> f64 {
    let value = doc.page(page).unwrap().sheet.cell(cell).unwrap().value.as_ref().unwrap();
    match value.reading() {
        Reading::Number(n) => n,
        other => panic!("{cell} reads as {other:?}"),
    }
}

// ---- the catalogue ----------------------------------------------------------------------------------------------------------------------

/// Every stencil that a template names is a stencil of the catalogue, shipped or not: a template is registered at once and listed when they all ship.
fn is_registered_stencil(id: &str) -> bool {
    stencil::registered().iter().any(|s| s.id == id)
}

#[test]
fn the_catalogue_registers_eighteen_templates_in_order_and_every_name_in_it_exists() {
    let ids: Vec<&str> = registered().iter().map(|t| t.id).collect();
    assert_eq!(
        ids,
        [
            "basic_diagram",
            "basic_flowchart",
            "cross_functional_flowchart",
            "organization_chart",
            "uml_class",
            "uml_use_case",
            "uml_sequence",
            "uml_activity",
            "uml_state_machine",
            "uml_component",
            "uml_deployment",
            "bpmn_diagram",
            "c4_diagram",
            "basic_network",
            "cloud_architecture",
            "crows_foot",
            "chen_notation",
            "gane_sarson",
        ]
    );
    let names: Vec<&str> = registered().iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "Basic Diagram",
            "Basic Flowchart",
            "Cross-Functional Flowchart",
            "Organization Chart",
            "UML Class",
            "UML Use Case",
            "UML Sequence",
            "UML Activity",
            "UML State Machine",
            "UML Component",
            "UML Deployment",
            "BPMN Diagram",
            "C4 Model Diagram",
            "Basic Network Diagram",
            "Cloud Architecture Diagram",
            "Crow's Foot Database Notation",
            "Chen's Database Notation",
            "Data Flow Model (Gane-Sarson)"
        ]
    );
    for t in registered() {
        assert!(!t.description.is_empty() && !t.category.is_empty(), "{}", t.id);
        assert!(size_preset(t.size).is_some(), "{}: the size", t.id);
        if let Some((theme_id, variant)) = t.theme {
            assert!(theme::find(theme_id).is_some() && variant <= 3, "{}: the theme", t.id);
        }
        assert!(!t.stencils.is_empty(), "{}: a template opens a stencil", t.id);
        for id in t.stencils {
            assert!(is_registered_stencil(id), "{}: stencil {id}", t.id);
        }
        let mut seen = t.stencils.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), t.stencils.len(), "{}: a stencil is named once", t.id);
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn the_fourteen_new_templates_open_the_stencils_of_the_design_in_the_shapes_window_order_and_say_what_they_are_based_on() {
    let want: [(&str, &[&str], &str); 14] = [
        ("uml_class", &["uml-class", "basic"], "UML 2.5.1"),
        ("uml_use_case", &["uml-usecase", "basic"], "UML 2.5.1"),
        ("uml_sequence", &["uml-sequence", "basic"], "UML 2.5.1"),
        ("uml_activity", &["uml-activity", "cross-functional", "basic"], "UML 2.5.1"),
        ("uml_state_machine", &["uml-state", "basic"], "UML 2.5.1"),
        ("uml_component", &["uml-component", "uml-class", "basic"], "UML 2.5.1"),
        ("uml_deployment", &["uml-component", "basic"], "UML 2.5.1"),
        ("bpmn_diagram", &["bpmn", "basic"], "BPMN 2.0.2"),
        ("c4_diagram", &["c4", "basic"], "C4 model"),
        ("basic_network", &["network", "computers", "basic"], "network"),
        ("cloud_architecture", &["cloud", "network", "computers"], "cloud"),
        ("crows_foot", &["crows-foot", "basic"], "crow's foot"),
        ("chen_notation", &["chen", "basic"], "Chen's"),
        ("gane_sarson", &["gane-sarson", "basic"], "Gane-Sarson"),
    ];
    for (id, stencils, notation) in want {
        let t = registered().iter().find(|t| t.id == id).unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(t.stencils, stencils, "{id}");
        assert!(t.seed.is_none() && t.theme.is_none(), "{id}: a blank page with no theme");
        assert_eq!((t.size, t.orientation), ("letter", Orientation::Landscape), "{id}");
        assert!(t.description.contains("based on"), "{id}: {}", t.description);
        assert!(t.description.contains(notation), "{id} names its notation `{notation}`: {}", t.description);
        assert!(["Software and Database", "Flowchart", "Network"].contains(&t.category), "{id}: {}", t.category);
    }
    // The categories of the design: the software notations, BPMN under Flowchart, the network kinds under Network.
    let category = |id: &str| registered().iter().find(|t| t.id == id).map(|t| t.category);
    assert_eq!(category("bpmn_diagram"), Some("Flowchart"));
    assert_eq!(category("basic_network"), Some("Network"));
    assert_eq!(category("cloud_architecture"), Some("Network"));
    assert_eq!(category("uml_class"), Some("Software and Database"));
    assert_eq!(category("gane_sarson"), Some("Software and Database"));
}

#[test]
fn a_template_is_listed_only_when_every_stencil_it_names_is_shipped() {
    for t in registered() {
        let ships = t.stencils.iter().all(|id| stencil::builtin_stencil(id).is_some());
        assert_eq!(templates().iter().any(|listed| listed.id == t.id), ships, "{}", t.id);
        assert_eq!(find(t.id).is_some(), ships, "{}: `find` finds the listed ones only", t.id);
    }
    for t in templates() {
        assert!(!t.stencils.is_empty() && t.stencils.iter().all(|id| stencil::builtin_stencil(id).is_some()), "{}", t.id);
    }
    // The order of the listing is the order of the registration.
    let registered_order: Vec<&str> = registered().iter().map(|t| t.id).filter(|id| templates().iter().any(|t| t.id == *id)).collect();
    assert_eq!(templates().iter().map(|t| t.id).collect::<Vec<_>>(), registered_order);
}

#[test]
fn the_gane_sarson_template_is_listed_and_opens_gane_sarson_then_basic() {
    let t = find("gane_sarson").expect("the Gane-Sarson stencil ships today, so its template is listed");
    assert_eq!((t.name, t.stencils), ("Data Flow Model (Gane-Sarson)", &["gane-sarson", "basic"][..]));
    assert!(templates().iter().any(|t| t.id == "gane_sarson"));
    assert_eq!(find("Data Flow Model (Gane-Sarson)").map(|t| t.id), Some("gane_sarson"));
    // A drawing started from it has those two stencils open, in that order, and the names are read back from the template's own part.
    let built = build_one(t);
    assert_eq!(stencil_names(&built.document).map(|n| n.ids), Some(vec!["gane-sarson", "basic"]));
    let mut s = crate::Session::new();
    let started = s.execute("file.new", &serde_json::json!({ "template": "gane_sarson" })).unwrap();
    assert_eq!(started["stencils"]["open"], serde_json::json!(["gane-sarson", "basic"]), "{started}");
    assert_eq!(s.stencils().open_builtins(), ["gane-sarson", "basic"]);
    // The listing a window reads has it, with its page and its stencils.
    let listed = s.execute("template.list", &serde_json::json!({})).unwrap();
    let entry = listed["templates"].as_array().unwrap().iter().find(|t| t["id"] == "gane_sarson").expect("listed");
    assert_eq!(entry["stencils"], serde_json::json!([{ "id": "gane-sarson", "name": "Gane-Sarson" }, { "id": "basic", "name": "Basic Shapes" }]));
    assert_eq!(entry["page"]["orientation"], "landscape");
}

#[test]
fn a_template_whose_stencils_are_not_all_shipped_is_not_listed_and_file_new_says_it_is_no_template() {
    let mut s = crate::Session::new();
    let listed = s.execute("template.list", &serde_json::json!({})).unwrap();
    let listed_ids: Vec<&str> = listed["templates"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str()).collect();
    let mut checked = 0;
    for t in registered() {
        if t.stencils.iter().all(|id| stencil::builtin_stencil(id).is_some()) {
            continue;
        }
        checked += 1;
        assert!(!listed_ids.contains(&t.id), "{} is not listed", t.id);
        for key in [t.id, t.name] {
            let before = s.document().cloned();
            let refused = s.execute("file.new", &serde_json::json!({ "template": key })).expect_err("an unlisted template is no template");
            let said = refused.to_string();
            assert!(said.contains("no template") && said.contains("template.list"), "{key}: {said}");
            assert_eq!(s.document().cloned(), before, "{key}: the open drawing is left as it is");
        }
    }
    // Until a family ships, the thirteen templates of the second batch wait for their stencils (the Gane-Sarson one is shown: its stencil ships today).
    assert!(checked > 0 || registered().iter().all(|t| t.stencils.iter().all(|id| stencil::builtin_stencil(id).is_some())));
    // An id that is no template at all says the same.
    let unknown = s.execute("file.new", &serde_json::json!({ "template": "no_such_template" })).expect_err("no template").to_string();
    assert!(unknown.contains("no template"), "{unknown}");
}

#[test]
fn a_template_is_found_by_its_id_or_its_name() {
    for key in ["basic_flowchart", "Basic Flowchart", "  BASIC-FLOWCHART ", "basic flowchart"] {
        assert_eq!(find(key).map(|t| t.id), Some("basic_flowchart"), "{key:?}");
    }
    assert_eq!(find("basic_diagram").map(|t| t.name), Some("Basic Diagram"));
    for key in ["", "basic", "flowchart", "Basic Flowchart.vstx", "../basic_diagram", &"b".repeat(65)] {
        assert!(find(key).is_none(), "{key:?}");
    }
}

// ---- building ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn each_template_is_a_package_that_reads_back_as_the_document_with_nothing_odd() {
    for spec in templates() {
        let built = build_one(spec);
        assert!(built.warnings.is_empty(), "{}: {:?}", spec.id, built.warnings);
        let (doc, report) = read(&built.bytes, &Limits::default()).unwrap();
        assert_eq!(report.variant, Some(Variant { kind: Kind::Template, macro_enabled: false }), "{}", spec.id);
        assert!(report.warnings.is_empty(), "{}: {:?}", spec.id, report.warnings);
        assert_eq!(doc, built.document, "{}: the document returned is what the bytes read as", spec.id);
        assert_eq!(&*doc.title, "Untitled", "{}: a drawing made from it is untitled", spec.id);
        assert_eq!(doc.pages.len(), 1, "{}: one page", spec.id);
        assert_eq!(&*doc.pages.iter().next().unwrap().name, "Page-1");
        // A template that seeds its page with an org chart has dropped one shape: the master of it is copied in, and no other.
        // The lanes of a cross-functional flowchart are made by the lanes' commands, from no master.
        let seeded = usize::from(spec.seed == Some(Seed::OrgChart));
        assert_eq!(doc.masters.len(), seeded, "{}: the stencils hold the masters until a shape is dropped", spec.id);
        assert_eq!(stencil_names(&doc).map(|n| n.ids), Some(spec.stencils.to_vec()), "{}", spec.id);
    }
}

#[test]
fn a_template_is_the_same_bytes_each_time_it_is_made() {
    for spec in templates() {
        assert_eq!(build_one(spec).bytes, build_one(spec).bytes, "{}", spec.id);
    }
}

#[test]
fn the_page_is_the_size_and_orientation_the_template_says() {
    let diagram = build_one(find("basic_diagram").unwrap()).document;
    let id = diagram.pages.iter().next().unwrap().id;
    assert_eq!((number(&diagram, id, "PageWidth"), number(&diagram, id, "PageHeight")), (11.0, 8.5));
    let flowchart = build_one(find("basic_flowchart").unwrap()).document;
    let id = flowchart.pages.iter().next().unwrap().id;
    assert_eq!((number(&flowchart, id, "PageWidth"), number(&flowchart, id, "PageHeight")), (8.5, 11.0));
    assert_eq!(find("basic_flowchart").unwrap().orientation, Orientation::Portrait);
    // Drawn one to one in both.
    for doc in [&diagram, &flowchart] {
        let id = doc.pages.iter().next().unwrap().id;
        assert_eq!((number(doc, id, "PageScale"), number(doc, id, "DrawingScale")), (1.0, 1.0));
    }
}

#[test]
fn a_template_with_a_theme_has_its_part_its_style_and_its_page_cells_and_one_without_has_none() {
    let plain = build_one(find("basic_diagram").unwrap()).document;
    assert!(plain.themes.is_empty());
    assert!(theme::theme_style_id(&plain).is_none());
    let id = plain.pages.iter().next().unwrap().id;
    assert_eq!(theme::selection_of(&plain, id).map(|s| s.is_themed()), Some(false));

    let themed = build_one(find("basic_flowchart").unwrap()).document;
    let fernwood = theme::find("fernwood").unwrap();
    assert!(themed.theme_by_index(fernwood.scheme()).is_some(), "the theme's part is in the package");
    assert!(theme::theme_style_id(&themed).is_some());
    let id = themed.pages.iter().next().unwrap().id;
    let selection = theme::selection_of(&themed, id).unwrap();
    assert_eq!((selection.theme, selection.variation_colours, selection.variation_styles), (fernwood.scheme(), 0, 0));
}

// ---- the list of stencils ---------------------------------------------------------------------------------------------------------------

#[test]
fn the_list_is_read_by_id_in_order_without_repeats_and_a_name_that_is_not_an_id_is_counted_and_not_followed() {
    let names = parse("flowchart;basic");
    assert_eq!((names.ids, names.ignored), (vec!["flowchart", "basic"], 0));
    let names = parse(" BASIC ; Flowchart ;; ");
    assert_eq!((names.ids, names.ignored), (vec!["basic", "flowchart"], 0), "case and white space do not matter, empty entries are nothing");
    let names = parse("basic;basic;flowchart;basic");
    assert_eq!((names.ids, names.ignored), (vec!["basic", "flowchart"], 2), "a repeat is counted");
    let names = parse("C:\\Stencils\\Mine.vssx;/etc/passwd;file-1;basic");
    assert_eq!((names.ids, names.ignored), (vec!["basic"], 3), "a path, a file stencil's id and nothing built in");
    assert_eq!(parse(""), StencilNames::default());
    assert_eq!(parse(";;;"), StencilNames::default());
}

#[test]
fn the_list_is_bounded_whatever_the_cell_holds() {
    // Half a million entries: only the first 64 are looked at.
    let text = "nonsense;".repeat(500_000);
    let names = parse(&text);
    assert_eq!((names.ids.len(), names.ignored), (0, MAX_NAMES));
    // The built-in ids after the cut are not found.
    let text = format!("{}basic", "x;".repeat(MAX_NAMES));
    assert_eq!(parse(&text).ids.len(), 0, "the 65th entry is not looked at");
    // The first 4 KiB: a name past it is not read. 2,100 letters of two bytes run past it, so the second entry is cut off and the whole is one entry; the
    // same with 2,000 letters ends before the cut, and the second entry is read. (A cut inside a multi-byte letter is dropped and not split.)
    let past = parse(&format!("{};basic", "é".repeat(2_100)));
    assert_eq!((past.ids.len(), past.ignored), (0, 1), "`basic` is past the first 4 KiB");
    let within = parse(&format!("{};basic", "é".repeat(2_000)));
    assert_eq!((within.ids, within.ignored), (vec!["basic"], 1), "the same list that ends before the cut is read whole");
    for pad in 0..4 {
        let text = format!("{}{}", "a".repeat(4_095 - pad), "é".repeat(8));
        let _ = parse(&text);
    }
}

#[test]
fn any_text_gives_a_list_of_distinct_built_in_ids_and_a_count_within_the_bound() {
    // A deterministic stream of strings from an alphabet of the parts that matter: the ids, the separator, white space, a multi-byte letter.
    let pieces = ["basic", "flowchart", "Basic", "FLOWCHART", ";", ";", " ", "é", "😀", "\u{0}", "/", "file-1", "bas", "ic"];
    let mut seed: u64 = 0x5eed_cafe;
    let mut next = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) as usize
    };
    for _ in 0..2_000 {
        let text: String = (0..next() % 60).map(|_| pieces[next() % pieces.len()]).collect();
        let names = parse(&text);
        let mut seen = names.ids.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), names.ids.len(), "{text:?}");
        assert!(names.ids.iter().all(|id| stencil::builtin_stencil(id).is_some()), "{text:?}");
        assert!(names.ids.len() + names.ignored <= MAX_NAMES, "{text:?}");
    }
}

#[test]
fn a_document_with_no_list_names_nothing_one_with_an_empty_list_names_none_and_the_default_stencils_are_no_list() {
    let mut doc = Document::new_blank();
    assert_eq!(stencil_names(&doc), None);
    assert!(doc.diacraft.is_none());
    assert!(set_stencil_names(&mut doc, &[]));
    assert_eq!(stencil_names(&doc), Some(StencilNames::default()), "an empty list is a template that wants none");
    assert!(set_stencil_names(&mut doc, &["flowchart", "nope", "basic"]));
    assert_eq!(stencil_names(&doc).map(|n| (n.ids, n.ignored)), Some((vec!["flowchart", "basic"], 0)), "a name that is not built in is not written");
    // The stencils a new drawing has say nothing: the child goes, and the part with it when nothing else is in it.
    assert!(set_stencil_names(&mut doc, &["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]));
    assert_eq!(stencil_names(&doc), None);
    assert!(doc.diacraft.is_none(), "a part with nothing in it is no part");
    // Beside the Snap & Glue options: the list goes and they stay.
    assert!(doc.set_diacraft_child("snap", Some(&[("glue", "0")])));
    assert!(set_stencil_names(&mut doc, &["basic"]));
    assert!(set_stencil_names(&mut doc, &["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]));
    assert_eq!(stencil_names(&doc), None);
    assert!(doc.diacraft_child("snap").is_some(), "the other child is as it was");
}

#[test]
fn data_of_a_version_this_build_does_not_read_is_not_interpreted_and_not_changed() {
    use diacraft_doc::{DIACRAFT_URN, RawAttrs, RawElement, RawNode};
    let child =
        RawElement { name: "dc:stencils".into(), attrs: RawAttrs::from_pairs([("open".to_owned(), "flowchart".to_owned())]), children: Vec::new() };
    let data = RawElement {
        name: "dc:Data".into(),
        attrs: RawAttrs::from_pairs([("xmlns:dc".to_owned(), DIACRAFT_URN.to_owned()), ("dc:version".to_owned(), "99".to_owned())]),
        children: vec![RawNode::Element(child)],
    };
    let mut doc = Document::new_blank();
    doc.diacraft = Some(std::sync::Arc::new(data));
    let before = doc.diacraft.clone();
    assert_eq!(stencil_names(&doc), None, "a later build's list is not read");
    assert_eq!(stencils_opened_by(&doc), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
    assert!(!set_stencil_names(&mut doc, &["basic"]), "and not written over");
    assert_eq!(doc.diacraft, before);
}

#[test]
fn a_child_without_an_open_attribute_is_no_list() {
    let mut doc = Document::new_blank();
    assert!(doc.set_diacraft_child(STENCILS_CHILD, Some(&[("something_else", "basic")])));
    assert_eq!(stencil_names(&doc), None);
    assert_eq!(stencils_opened_by(&doc), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
}

#[test]
fn the_path_rule_is_the_one_place_that_says_what_a_file_is() {
    for path in ["a.vstx", "/x/y", "C:\\x", "./x", "..", "x.", "basic_flowchart.vstx"] {
        assert!(is_path(path), "{path:?}");
    }
    for word in ["", "nope", "basic_flowchrt", "Basic Flowchart", "basic-diagram", " x "] {
        assert!(!is_path(word), "{word:?}");
    }
    // No template's id or name is a path.
    for t in templates() {
        assert!(!is_path(t.id) && !is_path(t.name), "{}", t.id);
    }
}

#[test]
fn what_a_list_opens_is_its_ids_or_the_default_when_it_names_stencils_and_none_can_be_opened() {
    let default = vec!["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"];
    assert_eq!(parse("flowchart").opens(), ["flowchart"]);
    assert_eq!(parse("").opens(), Vec::<&str>::new(), "an empty list is a template that wants none");
    assert_eq!(parse("/etc/passwd;file-1").opens(), default, "names it cannot open: the stencils a new drawing has");
    assert_eq!(parse("/etc/passwd;flowchart").opens(), ["flowchart"]);
    let mut doc = Document::new_blank();
    assert_eq!(stencils_opened_by(&doc), default, "no cell");
    assert!(set_stencil_names(&mut doc, &["flowchart", "basic"]));
    assert_eq!(stencils_opened_by(&doc), ["flowchart", "basic"]);
}

// ---- the list of a drawing -----------------------------------------------------------------------------------------

#[test]
fn a_list_that_named_more_than_is_looked_at_says_so() {
    assert!(!parse("basic;flowchart").cut);
    assert!(!parse("").cut);
    assert!(parse(&"x;".repeat(MAX_NAMES + 1)).cut, "an entry past the 64th says something");
    assert!(!parse(&"x;".repeat(MAX_NAMES)).cut, "the 64 are the 64, and what follows them is nothing");
    assert!(!parse(&format!("{}{}", "x;".repeat(MAX_NAMES), ";; ;")).cut, "blank entries after them are nothing");
    assert!(parse(&format!("{}basic", "x;".repeat(MAX_NAMES))).cut, "a name after them is cut");
    // Past the first 4 KiB, the text is cut, and a cut inside a multi-byte letter is dropped and not split.
    assert!(parse(&format!("{};basic", "é".repeat(2_100))).cut);
    assert!(!parse(&format!("{};basic", "é".repeat(2_000))).cut);
    // Half a million entries are one cut, and nothing walks them.
    assert!(parse(&"nonsense;".repeat(500_000)).cut);
}

/// A drawing with masters of the stencils `ids` made as a drop makes them: the masters are the built-in ones, with their fixed `BaseID`.
fn drawing_with_masters_of(ids: &[&str]) -> Document {
    let mut doc = Document::new_blank();
    for id in ids {
        let spec = stencil::find_master(id).unwrap_or_else(|| panic!("no master {id}"));
        stencil::ensure_master(&mut doc, spec).unwrap();
    }
    doc
}

#[test]
fn the_stencils_the_masters_of_a_drawing_came_from_are_found_by_their_fixed_ids_in_the_order_of_the_catalogue() {
    assert!(stencils_of_masters(&Document::new_blank()).is_empty());
    assert_eq!(stencils_of_masters(&drawing_with_masters_of(&["network.router"])), ["network"]);
    // Whatever the order the masters came in, the answer is the catalogue's order (Basic Flowchart before Computers and Devices), each stencil once.
    let doc = drawing_with_masters_of(&["computers.server", "network.router", "network.switch", "process"]);
    assert_eq!(stencils_of_masters(&doc), ["flowchart", "network", "computers"]);
    // A master of the drawing's own, or of a stencil file, names none; so does one whose ids are not a built-in master's.
    let mut doc = Document::new_blank();
    let mut own = diacraft_doc::Master { id: 1, name: Some("Mine".into()), ..diacraft_doc::Master::default() };
    own.unique_id = Some(std::sync::Arc::from("{00000000-0000-4000-8000-000000000001}"));
    own.raw.push("BaseID", &"x".repeat(10_000));
    doc.add_master(own).unwrap();
    assert!(stencils_of_masters(&doc).is_empty());
}

#[test]
fn a_master_is_known_by_its_unique_id_when_its_base_id_is_not_there_and_never_by_a_name() {
    let mut doc = drawing_with_masters_of(&["network.router"]);
    let mut named_only =
        diacraft_doc::Master { id: 90, name: Some("Router".into()), name_u: Some("Router".into()), ..diacraft_doc::Master::default() };
    named_only.unique_id = None;
    doc.add_master(named_only).unwrap();
    assert_eq!(stencils_of_masters(&doc), ["network"], "the master that holds the ids says it, the one with a name only says nothing");
    let mut by_unique = Document::new_blank();
    let guid = stencil::find_master("computers.server").map(|m| m.guid());
    let Some(guid) = guid else { return };
    let mut m = diacraft_doc::Master { id: 1, ..diacraft_doc::Master::default() };
    m.unique_id = Some(std::sync::Arc::from(guid.to_ascii_lowercase().as_str()));
    by_unique.add_master(m).unwrap();
    assert_eq!(stencils_of_masters(&by_unique), ["computers"], "a `UniqueID` alone says it, in any case");
}

#[test]
fn a_drawing_opens_the_stencils_its_list_names_or_with_none_the_defaults_and_those_its_masters_came_from() {
    let default = vec!["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"];
    // No list, no masters: the stencils a new drawing has, and the answer does not say it was a list.
    let found = stencils_of_drawing(&Document::new_blank());
    assert_eq!((found.ids, found.ignored, found.cut, found.listed), (default.clone(), 0, false, false));
    // No list, masters of the network stencil: the defaults and that.
    let found = stencils_of_drawing(&drawing_with_masters_of(&["network.router"]));
    let mut with_network = default.clone();
    with_network.push("network");
    assert_eq!((found.ids, found.listed), (with_network.clone(), false));
    // A list is what is opened, whatever the masters are.
    let mut doc = drawing_with_masters_of(&["network.router"]);
    assert!(set_stencil_names(&mut doc, &["flowchart", "basic"]));
    let found = stencils_of_drawing(&doc);
    assert_eq!((found.ids, found.listed), (vec!["flowchart", "basic"], true));
    // An empty list opens none, as a template's does.
    assert!(doc.set_diacraft_child(STENCILS_CHILD, Some(&[("open", "")])));
    assert_eq!(stencils_of_drawing(&doc).ids, Vec::<&str>::new());
    // A list of names that cannot be opened is no list: the defaults and the masters', and the names are counted.
    assert!(doc.set_diacraft_child(STENCILS_CHILD, Some(&[("open", "/etc/passwd;file-1")])));
    let found = stencils_of_drawing(&doc);
    assert_eq!((found.ids, found.ignored, found.listed), (with_network, 2, false));
}
