//! The layering rules (design section 6). The rule tests are VectorCraft's, with DiaCraft's crate names.

use serde_json::Value;
use xtask::layers::{
    Class, Crate, Dep, DepKind, INTRA_LAYER_ORDER, TABLE, UI_CRATES, UI_EXCEPTIONS, UI_PREFIXES, Violation, chain_problems, check, check_with_chains,
    classify, describe, from_metadata, short_name, wasm_set,
};

fn c(name: &str, deps: &[(&str, DepKind, bool)]) -> Crate {
    Crate { name: name.into(), deps: deps.iter().map(|(n, k, w)| Dep { name: (*n).into(), kind: *k, workspace: *w }).collect() }
}

use DepKind::{Dev, Normal};

#[test]
fn clean_downward_graph_passes() {
    let g = [
        c("diacraft-units", &[("serde", Normal, false)]),
        c("diacraft-doc", &[("diacraft-units", Normal, true)]),
        c("diacraft-engine", &[("diacraft-doc", Normal, true), ("diacraft-testkit", Dev, true)]),
        c("diacraft-ui-egui", &[("diacraft-engine", Normal, true), ("egui", Normal, false)]),
        c("diacraft-cli", &[("diacraft-ui-egui", Normal, true)]),
    ];
    assert!(check(&g).is_empty(), "{:?}", check(&g));
}

#[test]
fn upward_dependency_is_a_violation() {
    let v = check(&[c("diacraft-doc", &[("diacraft-engine", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 1, to: 5, .. }]), "{v:?}");
    assert!(v[0].to_string().contains("strictly lower layers"));
}

#[test]
fn sideways_dependency_is_a_violation() {
    let v = check(&[c("diacraft-text", &[("diacraft-pathops", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 2, .. }]), "{v:?}");
}

#[test]
fn upward_dev_dependency_is_a_violation() {
    let v = check(&[c("diacraft-geom", &[("diacraft-doc", Dev, true)])]);
    assert!(matches!(v[..], [Violation::Upward { kind: Dev, .. }]), "{v:?}");
}

#[test]
fn self_dev_dependency_is_ignored() {
    assert!(check(&[c("diacraft-doc", &[("diacraft-doc", Dev, true)])]).is_empty());
}

#[test]
fn an_intra_layer_chain_allows_one_direction_only() {
    let chains: &[&[&str]] = &[&["units", "geom"]];
    let later_uses_earlier = [c("diacraft-geom", &[("diacraft-units", Normal, true)])];
    assert!(check_with_chains(&later_uses_earlier, chains).is_empty());
    let earlier_uses_later = [c("diacraft-units", &[("diacraft-geom", Normal, true)])];
    let v = check_with_chains(&earlier_uses_later, chains);
    assert!(matches!(v[..], [Violation::Upward { from: 0, to: 0, .. }]), "{v:?}");
    // Without that chain (`geom` and `units` are in no registered one), two crates of one layer may not use each other.
    assert!(!check(&later_uses_earlier).is_empty());
}

/// The first chain registered (`INTRA_LAYER_ORDER`): the formula crate reads units with `diacraft-units`, and nothing
/// else of layer 0 is open to it, nor is it open to units.
#[test]
fn the_formula_crate_may_use_units_and_no_other_crate_of_its_layer() {
    assert!(check(&[c("diacraft-formula", &[("diacraft-units", Normal, true)])]).is_empty());
    let v = check(&[c("diacraft-units", &[("diacraft-formula", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 0, to: 0, .. }]), "{v:?}");
    for other in ["diacraft-geom", "diacraft-color"] {
        let v = check(&[c("diacraft-formula", &[(other, Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 0, to: 0, .. }]), "{other}: {v:?}");
    }
}

/// The chains registered are exactly this list and no more, and they are sound (round 1): a chain added by mistake is a
/// dependency allowed by mistake.
#[test]
fn the_registered_chains_are_exactly_these_and_sound() {
    assert_eq!(INTRA_LAYER_ORDER.len(), 5, "{INTRA_LAYER_ORDER:?}");
    assert_eq!(INTRA_LAYER_ORDER[0], ["units", "formula"]);
    assert_eq!(INTRA_LAYER_ORDER[1], ["render", "svg"]);
    assert_eq!(INTRA_LAYER_ORDER[2], ["render", "pdf"]);
    assert_eq!(INTRA_LAYER_ORDER[3], ["text", "calc"]);
    assert_eq!(INTRA_LAYER_ORDER[4], ["container", "opc"]);
    assert_eq!(chain_problems(INTRA_LAYER_ORDER), []);
}

/// The ZIP and XML layer: `opc` reads and writes its archives through the standalone `container` crate, and that is
/// the one workspace crate either may use. The container uses nothing of the workspace, and no other standalone crate is opened to either.
#[test]
fn opc_may_use_the_container_and_no_other_workspace_crate() {
    assert!(check(&[c("diacraft-opc", &[("diacraft-container", Normal, true), ("diacraft-opc", Dev, true)])]).is_empty());
    let standalone = |krate: &str, dep: &str| {
        let v = check(&[c(krate, &[(dep, Normal, true)])]);
        assert!(matches!(&v[..], [Violation::StandaloneHasWorkspaceDep { krate: k, dep: d }] if k == krate && d == dep), "{krate} -> {dep}: {v:?}");
    };
    standalone("diacraft-container", "diacraft-opc");
    standalone("diacraft-container", "diacraft-units");
    standalone("diacraft-opc", "diacraft-metafile");
    standalone("diacraft-metafile", "diacraft-container");
    standalone("diacraft-opc", "diacraft-units");
    // As a development dependency too.
    let v = check(&[c("diacraft-container", &[("diacraft-opc", Dev, true)])]);
    assert!(matches!(&v[..], [Violation::StandaloneHasWorkspaceDep { .. }]), "{v:?}");
    // A layered crate of layer 0 does not reach the container by the chain, and one of a higher layer reaches it as it reaches any
    // standalone crate.
    let v = check(&[c("diacraft-units", &[("diacraft-container", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 0, to: 0, .. }]), "{v:?}");
    assert!(check(&[c("diacraft-vsdx", &[("diacraft-container", Normal, true)])]).is_empty());
}

/// The ShapeSheet XML codec is a layer 2 crate: it uses the document model (layer 1) and the container's XML tree (layer 0, standalone), the
/// two file formats of layer 3 use it, and it uses neither of them, nor another crate of its own layer.
#[test]
fn the_sheet_codec_uses_the_model_and_the_container_and_the_formats_use_it() {
    assert!(check(&[c("diacraft-sheetxml", &[("diacraft-doc", Normal, true), ("diacraft-container", Normal, true)])]).is_empty());
    assert!(check(&[c("diacraft-vsdx", &[("diacraft-sheetxml", Normal, true)])]).is_empty());
    assert!(check(&[c("diacraft-odg", &[("diacraft-sheetxml", Normal, true)])]).is_empty());
    let v = check(&[c("diacraft-sheetxml", &[("diacraft-vsdx", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 3, .. }]), "{v:?}");
    let v = check(&[c("diacraft-sheetxml", &[("diacraft-odg", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 3, .. }]), "{v:?}");
    let v = check(&[c("diacraft-sheetxml", &[("diacraft-calc", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 2, .. }]), "{v:?}");
    // The model (layer 1) does not use the codec.
    let v = check(&[c("diacraft-doc", &[("diacraft-sheetxml", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 1, to: 2, .. }]), "{v:?}");
}

/// `TEXTWIDTH` and `TEXTHEIGHT` lay the text out with the text crate: the evaluator uses it, and the text crate does not use the
/// evaluator.
#[test]
fn the_evaluator_may_use_the_text_crate_and_not_the_other_way() {
    assert!(check(&[c("diacraft-calc", &[("diacraft-text", Normal, true)])]).is_empty());
    let v = check(&[c("diacraft-text", &[("diacraft-calc", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 2, .. }]), "{v:?}");
    let v = check(&[c("diacraft-calc", &[("diacraft-pathops", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 2, to: 2, .. }]), "{v:?}");
}

/// The export writers use `render`, and `render` uses neither of them; the two writers do not use each other.
#[test]
fn the_export_writers_may_use_render_and_not_each_other() {
    for writer in ["diacraft-svg", "diacraft-pdf"] {
        assert!(check(&[c(writer, &[("diacraft-render", Normal, true)])]).is_empty(), "{writer}");
        let v = check(&[c("diacraft-render", &[(writer, Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 3, to: 3, .. }]), "{writer}: {v:?}");
    }
    let v = check(&[c("diacraft-pdf", &[("diacraft-svg", Normal, true)])]);
    assert!(matches!(v[..], [Violation::Upward { from: 3, to: 3, .. }]), "{v:?}");
}

/// A chain that orders nothing was passed in silence: a name not in `TABLE`, a crate that is not layered, crates of two layers,
/// one name twice, fewer than two names.
#[test]
fn a_chain_that_orders_nothing_is_a_violation() {
    let bad = |chains: &[&[&str]], reason: &str| {
        let v = check_with_chains(&[], chains);
        assert!(matches!(&v[..], [Violation::BadChain { reason: r, .. }] if r.contains(reason)), "{chains:?}: wants `{reason}`: {v:?}");
        assert!(v[0].to_string().contains("INTRA_LAYER_ORDER"));
    };
    bad(&[&["units", "no-such-crate"]], "not in TABLE");
    bad(&[&["unit", "formula"]], "not in TABLE");
    bad(&[&["diacraft-units", "formula"]], "not in TABLE");
    bad(&[&["units", "opc"]], "not a layered crate");
    bad(&[&["container", "formula"]], "not a layered crate");
    bad(&[&["container", "testkit"]], "not a layered crate");
    bad(&[&["units", "testkit"]], "not a layered crate");
    bad(&[&["units", "cli"]], "not a layered crate");
    bad(&[&["units", "doc"]], "different layers");
    bad(&[&["units", "geom", "units"]], "twice");
    bad(&[&["units"]], "at least two");
    bad(&[&[]], "at least two");
    // One good chain and one bad: the bad one is found.
    bad(&[&["units", "geom"], &["color", "calc"]], "different layers");
    // Good chains are none: of layered crates of one layer, or of standalone crates only.
    assert_eq!(check_with_chains(&[], &[&["units", "geom"], &["geom", "color"], &["units", "geom", "color", "formula"]]), []);
    assert_eq!(check_with_chains(&[], &[&["container", "opc"], &["metafile", "dxf"]]), []);
    assert_eq!(check_with_chains(&[], &[]), []);
    // A chain of standalone crates opens one direction only, and a pair both ways is a cycle as for layered crates.
    let chain: &[&[&str]] = &[&["metafile", "dxf"]];
    assert!(check_with_chains(&[c("diacraft-dxf", &[("diacraft-metafile", Normal, true)])], chain).is_empty());
    assert!(!check_with_chains(&[c("diacraft-metafile", &[("diacraft-dxf", Normal, true)])], chain).is_empty());
    let v = check_with_chains(&[], &[&["metafile", "dxf"], &["dxf", "metafile"]]);
    assert!(v.iter().any(|v| matches!(v, Violation::BadChain { reason, .. } if reason.contains("both ways"))), "{v:?}");
}

/// Two chains that order a pair both ways allow a dependency both ways: `units` may use `geom` by one and `geom` may use `units` by
/// the other, and the layer check passes a cycle.
#[test]
fn chains_that_order_a_pair_both_ways_are_a_violation() {
    let cycle: &[&[&str]] = &[&["units", "geom"], &["geom", "units"]];
    let both = [c("diacraft-units", &[("diacraft-geom", Normal, true)]), c("diacraft-geom", &[("diacraft-units", Normal, true)])];
    let v = check_with_chains(&both, cycle);
    assert!(v.iter().any(|v| matches!(v, Violation::BadChain { reason, .. } if reason.contains("both ways"))), "{v:?}");
    // Through a third: units before geom before color before units.
    let ring: &[&[&str]] = &[&["units", "geom"], &["geom", "color"], &["color", "units"]];
    let v = check_with_chains(&[], ring);
    assert!(v.iter().any(|v| matches!(v, Violation::BadChain { reason, .. } if reason.contains("both ways"))), "{v:?}");
    // The same chain twice orders nothing new, and is no conflict.
    assert!(check_with_chains(&[], &[&["units", "geom"], &["units", "geom"]]).is_empty());
    // And a chain with a name left in a typo does not allow what it was meant to.
    let typo: &[&[&str]] = &[&["units", "fromula"]];
    assert!(!check_with_chains(&[c("diacraft-formula", &[("diacraft-units", Normal, true)])], typo).is_empty());
}

#[test]
fn ui_crate_below_l6_is_a_violation() {
    assert_eq!(UI_CRATES, ["egui", "eframe", "egui_extras", "winit", "egui_kittest", "craft-ui", "craft-keymap", "craft-window", "rfd"]);
    assert_eq!(UI_PREFIXES, ["egui", "epaint", "wgpu"]);
    for dep in UI_CRATES {
        let v = check(&[c("diacraft-engine", &[(dep, Normal, false)])]);
        assert!(matches!(v[..], [Violation::UiBelowL6 { layer: 5, .. }]), "{dep}: {v:?}");
    }
    for fine in ["serde", "kurbo", "ecolor", "emath", "winit_not", "rf", "craft-fonts-not"] {
        assert!(check(&[c("diacraft-engine", &[(fine, Normal, false)])]).is_empty(), "{fine}");
    }
    assert!(check(&[c("diacraft-ui-egui", &[("craft-ui", Normal, false)])]).is_empty());
    assert!(check(&[c("diacraft-mcp", &[("winit", Normal, false)])]).is_empty());
}

/// `craft-window` is configured with `features = ["egui"]` in the workspace manifest, so it pulls egui
/// into whoever depends on it. The egui, epaint and wgpu families have many members, matched by prefix.
#[test]
fn craft_window_is_a_ui_crate() {
    for krate in ["diacraft-render", "diacraft-doc", "diacraft-tools", "diacraft-engine"] {
        let v = check(&[c(krate, &[("craft-window", Normal, false)])]);
        assert!(matches!(&v[..], [Violation::UiBelowL6 { dep, .. }] if dep == "craft-window"), "{krate}: {v:?}");
    }
    assert!(check(&[c("diacraft-ui-egui", &[("craft-window", Normal, false)])]).is_empty());
}

#[test]
fn egui_epaint_and_wgpu_family_crates_are_ui_crates() {
    let family = [
        // egui-
        "egui-winit",
        "egui-wgpu",
        // egui_
        "egui_glow",
        "egui_inspection",
        "egui_system_font_provider",
        "egui_demo_lib",
        // epaint
        "epaint",
        "epaint_default_fonts",
        // wgpu
        "wgpu",
        "wgpu-core",
        "wgpu-hal",
        "wgpu-types",
        "wgpu_types",
    ];
    for dep in family {
        for (krate, layer) in [("diacraft-units", 0), ("diacraft-render", 3), ("diacraft-tools", 4), ("diacraft-testkit", 6)] {
            let v = check(&[c(krate, &[(dep, Normal, false)])]);
            if layer < 6 {
                assert!(matches!(&v[..], [Violation::UiBelowL6 { dep: d, .. }] if d == dep), "{krate} -> {dep}: {v:?}");
            } else {
                assert!(v.is_empty(), "the testkit sits at L6: {krate} -> {dep}: {v:?}");
            }
        }
        assert!(check(&[c("diacraft-ui-egui", &[(dep, Normal, false)])]).is_empty(), "{dep}");
    }
    // A development dependency counts too.
    assert_eq!(check(&[c("diacraft-render", &[("egui-wgpu", Dev, false)])]).len(), 1);
}

/// The one exception to the UI rule: the font database takes Hack and Ubuntu Light, bytes and nothing else, from the data crate
/// `epaint_default_fonts`. It is that pair and no other, and every other UI crate stays banned for it.
#[test]
fn the_font_crate_may_use_the_font_data_crate_and_nothing_else_named_like_a_toolkit() {
    assert_eq!(UI_EXCEPTIONS, [("fonts", "epaint_default_fonts")]);
    assert!(check(&[c("diacraft-fonts", &[("epaint_default_fonts", Normal, false)])]).is_empty());
    for dep in ["epaint", "egui", "egui_glow", "epaint_default_fonts_not", "winit", "wgpu"] {
        let v = check(&[c("diacraft-fonts", &[(dep, Normal, false)])]);
        assert!(matches!(&v[..], [Violation::UiBelowL6 { dep: d, .. }] if d == dep), "fonts -> {dep}: {v:?}");
    }
    for krate in ["diacraft-text", "diacraft-doc", "diacraft-render", "diacraft-engine", "diacraft-units"] {
        let v = check(&[c(krate, &[("epaint_default_fonts", Normal, false)])]);
        assert!(matches!(&v[..], [Violation::UiBelowL6 { dep, .. }] if dep == "epaint_default_fonts"), "{krate}: {v:?}");
    }
}

#[test]
fn unregistered_crate_is_a_violation() {
    let v = check(&[c("diacraft-mystery", &[])]);
    assert!(matches!(&v[..], [Violation::Unregistered { krate }] if krate == "diacraft-mystery"), "{v:?}");
    assert!(v[0].to_string().contains("register"));
}

#[test]
fn standalone_with_workspace_dep_is_a_violation() {
    let v = check(&[c("diacraft-opc", &[("diacraft-units", Normal, true)])]);
    assert!(matches!(&v[..], [Violation::StandaloneHasWorkspaceDep { krate, dep }] if krate == "diacraft-opc" && dep == "diacraft-units"), "{v:?}");
    // A standalone crate may use outside crates, and even its own dev-dependency features.
    assert!(check(&[c("diacraft-opc", &[("zip", Normal, false), ("diacraft-opc", Dev, true)])]).is_empty());
}

#[test]
fn testkit_as_normal_dep_is_a_violation() {
    let v = check(&[c("diacraft-render", &[("diacraft-testkit", Normal, true)])]);
    assert!(matches!(v[..], [Violation::TestkitAsNormalDep { .. }]), "{v:?}");
    assert!(v[0].to_string().contains("diacraft-testkit"));
    assert!(check(&[c("diacraft-render", &[("diacraft-testkit", Dev, true)])]).is_empty());
    // The testkit itself may use anything up to L5, but not an L6 crate.
    assert!(check(&[c("diacraft-testkit", &[("diacraft-engine", Normal, true)])]).is_empty());
    assert!(!check(&[c("diacraft-testkit", &[("diacraft-ui-egui", Normal, true)])]).is_empty());
}

#[test]
fn apps_and_xtask_are_exempt() {
    for app in ["diacraft", "diacraft-cli", "diacraft-web", "xtask"] {
        assert!(check(&[c(app, &[("egui", Normal, false), ("diacraft-ui-egui", Normal, true)])]).is_empty(), "{app}");
    }
}

#[test]
fn metadata_parsing() {
    let meta: Value = serde_json::from_str(
        r#"{"packages":[
            {"name":"diacraft-doc","dependencies":[
                {"name":"diacraft-units","kind":null,"path":"/x/crates/units"},
                {"name":"serde","kind":null},
                {"name":"proptest","kind":"dev"}]},
            {"name":"diacraft-units","dependencies":[]}
        ]}"#,
    )
    .unwrap();
    let g = from_metadata(&meta).unwrap();
    assert_eq!(g.len(), 2);
    let doc = g.iter().find(|c| c.name == "diacraft-doc").unwrap();
    assert!(doc.deps[0].workspace && !doc.deps[1].workspace);
    assert_eq!(doc.deps[2].kind, Dev);
    assert!(check(&g).is_empty());
    assert!(from_metadata(&serde_json::json!({})).is_err());
}

/// Every crate of design section 6, by layer.
#[test]
fn the_table_registers_every_crate_of_design_section_6() {
    let layers: [(u8, &[&str]); 6] = [
        (0, &["units", "geom", "color", "formula"]),
        (1, &["doc", "fonts"]),
        // `sheetxml` is the ShapeSheet XML codec carved out of `vsdx`; design section 6 predates it.
        (2, &["calc", "text", "pathops", "layout", "plugins", "sheetxml"]),
        // `odg` is a file format like the rest of this layer, though design section 6 predates it; so is `native`, DiaCraft's own.
        (3, &["render", "vsdx", "odg", "native", "vsd", "vdx", "svg", "pdf", "pptx", "emf", "cadimport", "interchange"]),
        (4, &["tools"]),
        (5, &["engine"]),
    ];
    for (layer, names) in layers {
        for name in names {
            assert_eq!(classify(&format!("diacraft-{name}")), Some(Class::Layer(layer)), "{name}");
        }
    }
    for name in ["ui-egui", "mcp"] {
        assert_eq!(classify(&format!("diacraft-{name}")), Some(Class::Layer(6)), "{name}");
    }
    // `container` is not in design section 6: the ZIP and XML layer carved out of `opc`.
    for name in ["container", "opc", "metafile", "dxf", "data"] {
        assert_eq!(classify(&format!("diacraft-{name}")), Some(Class::Standalone), "{name}");
    }
    assert_eq!(classify("diacraft-testkit"), Some(Class::Testkit));
    for name in ["diacraft", "diacraft-cli", "diacraft-web", "xtask"] {
        assert_eq!(classify(name), Some(Class::Exempt), "{name}");
    }
    // Nothing else is registered, and nothing twice.
    let registered = layers.iter().map(|(_, n)| n.len()).sum::<usize>() + 2 + 5 + 1 + 4;
    assert_eq!(TABLE.len(), registered, "{TABLE:?}");
    let mut names: Vec<&str> = TABLE.iter().map(|(n, _)| *n).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), TABLE.len(), "a crate is registered twice");
}

#[test]
fn names_and_descriptions() {
    assert_eq!(short_name("diacraft-ui-egui"), "ui-egui");
    assert_eq!(short_name("diacraft"), "diacraft");
    assert_eq!(describe(classify("diacraft-calc")), "L2");
    assert_eq!(describe(classify("diacraft-dxf")), "L0 standalone");
    assert_eq!(describe(classify("diacraft-testkit")), "testkit");
    assert_eq!(describe(classify("xtask")), "exempt");
    assert_eq!(describe(classify("nope")), "UNREGISTERED");
}

/// The test below shows the rule on made-up crates; this one shows the repository: the crates `cargo xtask wasm` checks
/// include the web app and the window it runs, so the gate cannot pass without compiling `diacraft-web` for the browser,
/// and `cargo xtask layers` accepts the web app (L7, exempt) with what it depends on.
#[test]
fn the_wasm_step_checks_the_web_app_in_this_repository() {
    let crates = from_metadata(&xtask::metadata().unwrap()).unwrap();
    let set = wasm_set(&crates);
    for name in ["diacraft-doc", "diacraft-engine", "diacraft-ui-egui", "diacraft-web"] {
        assert!(set.iter().any(|n| n == name), "`cargo xtask wasm` does not check {name}: {set:?}");
    }
    assert!(crates.iter().any(|c| c.name == "diacraft-web" && classify(&c.name) == Some(Class::Exempt)));
    let violations: Vec<String> = check(&crates).iter().map(ToString::to_string).collect();
    assert!(violations.is_empty(), "layering violations: {violations:?}");
}

#[test]
fn the_wasm_set_is_layers_0_to_5_standalone_ui_egui_and_web() {
    let names = [
        "diacraft-units",
        "diacraft-opc",
        "diacraft-engine",
        "diacraft-ui-egui",
        "diacraft-mcp",
        "diacraft-testkit",
        "diacraft",
        "diacraft-cli",
        "diacraft-web",
        "xtask",
        "diacraft-mystery",
    ];
    let crates: Vec<Crate> = names.iter().map(|n| c(n, &[])).collect();
    assert_eq!(wasm_set(&crates), ["diacraft-units", "diacraft-opc", "diacraft-engine", "diacraft-ui-egui", "diacraft-web"]);
    // Nothing to check until such a crate exists.
    assert!(wasm_set(&[]).is_empty());
    assert!(wasm_set(&[c("xtask", &[]), c("diacraft-testkit", &[]), c("diacraft-cli", &[])]).is_empty());
}
