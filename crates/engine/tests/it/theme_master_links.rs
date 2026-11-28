//! A theme reaches an instance through its master (the owner's ruling of 2026-11-27, "Link masters to the theme").
//!
//! The VSDX application's check of DiaCraft's files: after `theme.apply`, every instance of the connector masters of the built-in stencils read `BeginArrow` 0, `EndArrow` 0, `LinePattern` 1
//! and `FillPattern` 1 from `THEMEVAL()` there, a solid line with no ends, whatever its master held. DiaCraft linked each shape the theme reached to the `Theme` style for its three families,
//! and the application reads an instance's own link before its master's cells of that family. The files it writes link their masters' shapes to the `Theme` style and leave the instances with
//! none. So a theme now links the master shapes that the page's instances stand for, and an instance keeps no link of its own; a shape with no master keeps
//! its own link, as before.
//!
//! **DiaCraft draws what it drew.** With no link of its own an instance follows its master shape's (R32 step 3); the `Theme` style's formulas are worked out on the instance, with its own
//! page's theme (65534 reads the page), so one master serves pages of different themes, and on a page that selects none a cell that follows the
//! style reads the root style's. The pictures here are compared, pixel for pixel, with the drawing as the earlier rule left it: each instance linked to the
//! `Theme` style itself, and its master shape on the root.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Document, Links, Owner, Shape, ShapeList, ShapeRef, ShapeType};
use diacraft_engine::Session;
use diacraft_engine::theme::theme_style_id;
use diacraft_opc::{Limits, Package};
use diacraft_render::{Renderer, View};
use serde_json::{Value as Json, json};

use crate::edit::{doc, id_of};
use crate::edit_foreign::Dir;
use crate::master_line_cells::{LINKS, LOOK, PAGE_PART, attr, drop_all, master_shape, masters_of, part, shape_in, styles};
use crate::{blank_session, check_session, run};

/// The five themes of the catalogue.
const THEMES: [&str; 5] = ["tidepool", "fernwood", "cinderglow", "inkstone", "plumline"];

/// The pixels per inch the pages are drawn at: enough to tell a line end, a dash and a colour apart.
const PPI: f64 = 40.0;

// ---- the model ----------------------------------------------------------------------------------------------------------------------------------------------------------------

/// The `ID` of the drawing's `Theme` style.
fn theme_id(d: &Document) -> u32 {
    theme_style_id(d).filter(|id| *id != 0).expect("the drawing has a Theme style")
}

/// A shape of a page that stands for a shape of a master: an instance (the master's first top-level shape) or a member of one (`MasterShape`).
#[derive(Clone, Debug)]
struct Stands {
    /// Where the shape is, and its `ID`.
    at: ShapeRef,
    id: u32,
    /// Its own links.
    own: Links,
    /// The master, the path of the master's shape in it, and that shape's links.
    master: u32,
    path: Vec<usize>,
    theirs: Links,
}

/// The path and the links of the shape of master `master` that a shape stands for: the first top-level shape that is not deleted, or the shape whose `ID` is `id`.
fn master_shape_of(d: &Document, master: u32, id: Option<u32>) -> Option<(Vec<usize>, Links)> {
    let m = d.master(master)?;
    let Some(id) = id else {
        let (k, shape) = m.shapes.iter().enumerate().find(|(_, s)| !s.del)?;
        return Some((vec![k], shape.sheet.links));
    };
    let mut work: Vec<(Vec<usize>, &Shape)> = m.shapes.iter().enumerate().map(|(k, s)| (vec![k], &**s)).collect();
    while let Some((path, shape)) = work.pop() {
        if shape.id == id {
            return Some((path, shape.sheet.links));
        }
        for (k, child) in shape.children.iter().enumerate() {
            let mut deeper = path.clone();
            deeper.push(k);
            work.push((deeper, &**child));
        }
    }
    None
}

/// The style links shape `id` of page `page` follows, family by family: its own, else those of the master's shape it stands for (step 3).
pub(crate) fn follows(d: &Document, page: u32, id: u32) -> (Option<u32>, Option<u32>, Option<u32>) {
    let at = d.find_shape(Owner::Page(page), id).expect("the shape");
    let own = d.shape(&at).expect("the shape").sheet.links;
    let theirs = diacraft_doc::Resolver::new(d)
        .master_step(&at)
        .ok()
        .flatten()
        .and_then(|step| d.shape(&step.shape).map(|m| m.sheet.links))
        .unwrap_or_default();
    (own.line.or(theirs.line), own.fill.or(theirs.fill), own.text.or(theirs.text))
}

/// Whether a shape draws (not a group, a guide or a picture): what a theme makes follow its style.
fn draws(shape: &Shape) -> bool {
    match shape.kind {
        ShapeType::Shape => true,
        ShapeType::Unspecified => !shape.del,
        _ => false,
    }
}

/// Every shape of page `page` that draws and stands for a shape of a master, in document order.
fn standing(d: &Document, page: u32) -> Vec<Stands> {
    let mut out = Vec::new();
    let top = &d.page(page).expect("the page").shapes;
    let mut work: Vec<(ShapeRef, &Shape, Option<u32>)> =
        top.iter().enumerate().rev().map(|(k, s)| (ShapeRef::top(Owner::Page(page), k as u32), &**s, None)).collect();
    while let Some((at, shape, above)) = work.pop() {
        let found = match (shape.master, shape.master_shape, above) {
            (Some(m), _, _) => Some((m, None)),
            (None, Some(id), Some(m)) => Some((m, Some(id))),
            _ => None,
        };
        if let Some((m, id)) = found
            && draws(shape)
            && let Some((path, theirs)) = master_shape_of(d, m, id)
        {
            out.push(Stands { at: at.clone(), id: shape.id, own: shape.sheet.links, master: m, path, theirs });
        }
        let master = shape.master.or(above);
        for (k, child) in shape.children.iter().enumerate().rev() {
            work.push((at.child(k as u32), &**child, master));
        }
    }
    out
}

/// The name a shape is told by in a message: its `NameU`, else its `ID`.
fn named(d: &Document, at: &ShapeRef) -> String {
    d.shape(at).and_then(|s| s.name_u.as_deref().map(str::to_owned)).unwrap_or_else(|| format!("shape {at:?}"))
}

/// Asserts that each shape of page `page` that stands for a master's shape holds no link of its own, and that the master's shape links the `Theme` style for its three families.
#[track_caller]
fn assert_through_masters(d: &Document, page: u32) {
    let theme = Some(theme_id(d));
    let mut wrong = Vec::new();
    let found = standing(d, page);
    for s in &found {
        if s.own != Links::default() {
            wrong.push(format!("{}: holds links of its own {:?}", named(d, &s.at), s.own));
        }
        if (s.theirs.line, s.theirs.fill, s.theirs.text) != (theme, theme, theme) {
            wrong.push(format!("{}: its master's shape links {:?}, not the Theme style {theme:?}", named(d, &s.at), s.theirs));
        }
    }
    assert!(!found.is_empty(), "page {page} has instances");
    assert!(wrong.is_empty(), "{} of {} shapes of page {page}:\n{}", wrong.len(), found.len(), wrong.join("\n"));
}

/// The drawing as the earlier rule left it: each shape of `pages` that stands for a master's shape linked to the `Theme` style itself, for each family its master's shape links
/// to that style, and every master's shape back on the root style for it.
fn as_linked_on_their_own(d: &Document, pages: &[u32]) -> Document {
    let theme = theme_id(d);
    let mut out = d.clone();
    let mut masters: Vec<(u32, Vec<usize>, Links)> = Vec::new();
    for &page in pages {
        for s in standing(d, page) {
            let give = |own: Option<u32>, theirs: Option<u32>| if own.is_none() && theirs == Some(theme) { Some(theme) } else { own };
            let links = Links { line: give(s.own.line, s.theirs.line), fill: give(s.own.fill, s.theirs.fill), text: give(s.own.text, s.theirs.text) };
            let page_at = out.pages.iter().position(|p| p.id == page).unwrap();
            let p = Arc::make_mut(out.pages.get_mut(page_at).unwrap());
            let shape = shape_at_mut(&mut p.shapes, &s.at.path);
            Arc::make_mut(&mut shape.sheet).links = links;
            masters.push((s.master, s.path.clone(), s.theirs));
        }
    }
    for (master, path, theirs) in masters {
        let back = |l: Option<u32>| if l == Some(theme) { Some(0) } else { l };
        let at = out.masters.iter().position(|m| m.id == master).unwrap();
        let m = Arc::make_mut(out.masters.get_mut(at).unwrap());
        let shape = shape_at_mut_usize(&mut m.shapes, &path);
        Arc::make_mut(&mut shape.sheet).links = Links { line: back(theirs.line), fill: back(theirs.fill), text: back(theirs.text) };
    }
    out
}

fn shape_at_mut<'a>(list: &'a mut ShapeList, path: &[u32]) -> &'a mut Shape {
    let path: Vec<usize> = path.iter().map(|k| *k as usize).collect();
    shape_at_mut_usize(list, &path)
}

fn shape_at_mut_usize<'a>(list: &'a mut ShapeList, path: &[usize]) -> &'a mut Shape {
    let (first, rest) = path.split_first().unwrap();
    let shape = Arc::make_mut(list.get_mut(*first).unwrap());
    if rest.is_empty() { shape } else { shape_at_mut_usize(&mut shape.children, rest) }
}

/// Page `page` of a drawing, drawn.
fn drawn(d: &Document, page: u32) -> Vec<u8> {
    let view = View::fit_page(d, page, PPI).expect("the page has a size");
    let r = Renderer::new().render(d, page, &view);
    assert!(!r.rgba_premul.is_empty(), "page {page} was drawn: {:?}", r.skipped);
    r.rgba_premul
}

/// Asserts that page `page` of `d` and of `old` are the same picture, pixel for pixel, and says how many pixels differ and where the first is.
#[track_caller]
fn assert_same_picture(d: &Document, old: &Document, page: u32, what: &str) {
    let (new, was) = (drawn(d, page), drawn(old, page));
    assert_eq!(new.len(), was.len(), "{what}: the pictures are the same size");
    let differ: Vec<usize> = (0..new.len() / 4).filter(|k| new[k * 4..k * 4 + 4] != was[k * 4..k * 4 + 4]).collect();
    let width = View::fit_page(d, page, PPI).unwrap().width as usize;
    assert!(
        differ.is_empty(),
        "{what}: {} pixels of page {page} differ, the first at ({}, {}): {:?} against {:?}",
        differ.len(),
        differ[0] % width,
        differ[0] / width,
        &new[differ[0] * 4..differ[0] * 4 + 4],
        &was[differ[0] * 4..differ[0] * 4 + 4]
    );
}

/// Drops every master of the shipped stencils on page `page`, on a grid, each from its own stencil. → the key and the `ID` of each instance made. A master that cannot be dropped on
/// a plain page (none should) fails the test.
fn drop_every(s: &mut Session, page: u32) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut k = 0usize;
    for stencil in diacraft_engine::stencil::stencils() {
        for master in stencil.masters() {
            let (x, y) = (0.8 + (k % 10) as f64 * 1.05, 0.6 + ((k / 10) % 8) as f64 * 1.0);
            k += 1;
            let key = format!("{}/{}", stencil.id, master.id);
            let made = s
                .execute("shape.drop", &json!({ "master": master.id, "stencil": stencil.id, "page": page, "x": x, "y": y }))
                .unwrap_or_else(|e| panic!("{key}: {e}"));
            out.push((key, id_of(&made)));
        }
    }
    assert!(out.len() >= 100, "the catalogue's masters: {}", out.len());
    out
}

/// The look `shape.formatting` reads for shape `id` of page `page`.
fn look(s: &mut Session, page: u32, id: u32) -> Json {
    run(s, "shape.formatting", json!({ "page": page, "ids": [id] }))["shapes"][0].clone()
}

#[test]
fn a_theme_applied_to_a_page_links_the_master_shapes_its_instances_stand_for_and_no_instance() {
    let mut s = blank_session();
    drop_every(&mut s, 1);
    run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    assert_through_masters(doc(&s), 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_dropped_on_a_themed_page_follows_the_theme_through_its_master() {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({ "theme": "fernwood" }));
    drop_every(&mut s, 1);
    assert_through_masters(doc(&s), 1);
    // A second drop of a master the theme reaches already changes no master and gives the instance no link.
    let again = id_of(&run(&mut s, "shape.drop", json!({ "master": "uml-class.generalization", "page": 1, "x": 5.0, "y": 8.0 })));
    let d = doc(&s);
    let shape = d.shape(&d.find_shape(Owner::Page(1), again).unwrap()).unwrap();
    assert_eq!(shape.sheet.links, Links::default());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn only_a_drop_of_a_master_the_theme_does_not_reach_yet_looks_at_the_drawing_for_its_other_shapes() {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    let walks = |s: &mut Session, master: &str| {
        diacraft_engine::theme::reset_follow_work();
        run(s, "shape.drop", json!({ "master": master, "page": 1, "x": 4.0, "y": 4.0 }));
        diacraft_engine::theme::follow_work().master_walks
    };
    assert_eq!(walks(&mut s, "process"), 1, "a master new to the drawing: one walk, to find what else stands for it");
    for _ in 0..3 {
        assert_eq!(walks(&mut s, "process"), 0, "the master follows the style already: no walk");
    }
    // A theme applied again finds nothing to link: no walk, no change.
    diacraft_engine::theme::reset_follow_work();
    let r = run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    assert_eq!((r["shapes_linked"].clone(), r["changed_pages"].clone()), (json!(0), json!([])), "{r}");
    assert_eq!(diacraft_engine::theme::follow_work().master_walks, 0);
}

#[test]
fn a_new_drawing_from_a_template_with_a_theme_links_the_masters_of_the_shapes_dropped_on_it() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "basic_flowchart" }));
    let page = doc(&s).pages.get(0).unwrap().id;
    for (k, master) in ["process", "decision", "terminator", "rectangle"].iter().enumerate() {
        run(&mut s, "shape.drop", json!({ "master": master, "page": page, "x": 1.5 + k as f64 * 1.5, "y": 3.0 }));
    }
    assert_through_masters(doc(&s), page);
}

#[test]
fn a_shape_with_no_master_keeps_its_own_link_to_the_theme() {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    let a = id_of(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 2.0, "y": 2.0 })));
    let b = id_of(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 6.0, "y": 4.0 })));
    let c = id_of(&run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } })));
    let t = id_of(&run(&mut s, "text.box", json!({ "page": 1, "x": 3.0, "y": 6.0, "text": "A note" })));
    let d = doc(&s);
    let theme = Some(theme_id(d));
    for id in [c, t] {
        let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
        assert_eq!(shape.master, None);
        assert_eq!((shape.sheet.links.line, shape.sheet.links.fill, shape.sheet.links.text), (theme, theme, theme), "shape {id}");
    }
}

// ---- the files ----------------------------------------------------------------------------------------------------------------------------------------------------------------

/// Where the VSDX application finds `cell` for an instance, by what its check showed: the instance's own cell; else, when the instance links a style of its own for the cell's family
/// (`link`), that style's chain; else the master's shape's own cell; else the chain of the style the master's shape links. → where, and the cell's start tag (`None` past them all).
fn found_in_file(
    (tag, own): (&str, &BTreeMap<String, String>),
    (mtag, mine): (&str, &BTreeMap<String, String>),
    styles: &crate::master_line_cells::Styles,
    (cell, link): (&str, &str),
) -> (String, Option<String>) {
    let chain = |mut id: String| -> (String, Option<String>) {
        for _ in 0..16 {
            let Some((stag, cells)) = styles.get(&id) else { break };
            if let Some(c) = cells.get(cell) {
                return (format!("style {id}"), Some(c.clone()));
            }
            match attr(stag, link) {
                Some(next) if next != id => id = next.to_owned(),
                _ => break,
            }
        }
        ("nowhere".to_owned(), None)
    };
    if let Some(c) = own.get(cell) {
        return ("its own cell".to_owned(), Some(c.clone()));
    }
    if let Some(style) = attr(tag, link) {
        return chain(style.to_owned());
    }
    if let Some(c) = mine.get(cell) {
        return ("the master's cell".to_owned(), Some(c.clone()));
    }
    match attr(mtag, link) {
        Some(style) => chain(style.to_owned()),
        None => chain("0".to_owned()),
    }
}

/// Asserts that every instance of the file at `path` takes the theme through its master as the VSDX application reads it: no link of its own, its master's shape linked to the `Theme`
/// style for the three families, the master's line ends, pattern and fill read from the master, and the line colour read from the `Theme` style's `THEMEVAL()`.
#[track_caller]
fn assert_themed_through_masters(path: &str, instances: &[(&'static str, u32, u32)]) {
    let pkg = Package::read(&std::fs::read(path).unwrap(), &Limits::default()).unwrap();
    let page = part(&pkg, PAGE_PART);
    let styles = styles(&pkg);
    let theme =
        styles.iter().find(|(_, (tag, _))| attr(tag, "NameU") == Some("Theme")).map(|(id, _)| id.clone()).expect("the file has a Theme style");
    let mut wrong = Vec::new();
    for &(name, id, master) in instances {
        let (tag, own) = shape_in(&page, id).unwrap_or_else(|| panic!("{name}: no shape {id} in the page"));
        assert_eq!(attr(&tag, "Master"), Some(master.to_string().as_str()), "{name}: {tag}");
        let (mtag, mine) = master_shape(&pkg, master);
        for link in LINKS {
            if let Some(n) = attr(&tag, link) {
                wrong.push(format!("{name}: the instance links style {n} for {link} of its own"));
            }
            if attr(&mtag, link) != Some(theme.as_str()) {
                wrong.push(format!("{name}: the master's shape links {:?} for {link}, not the Theme style {theme}", attr(&mtag, link)));
            }
        }
        for (cell, link) in LOOK {
            let Some(want) = mine.get(cell).and_then(|c| attr(c, "V")) else { continue };
            let (from, got) = found_in_file((&tag, &own), (&mtag, &mine), &styles, (cell, link));
            if got.as_deref().and_then(|c| attr(c, "V")) != Some(want) {
                wrong.push(format!("{name}: {cell} is {want} on the master, and the application reads {got:?} from {from}"));
            }
        }
        if !mine.contains_key("LineColor") {
            let (from, got) = found_in_file((&tag, &own), (&mtag, &mine), &styles, ("LineColor", "LineStyle"));
            let themed = got.as_deref().is_some_and(|c| attr(c, "F") == Some("THEMEVAL()") && attr(c, "V") == Some("Themed"));
            if from != format!("style {theme}") || !themed {
                wrong.push(format!("{name}: the line colour is read from {from}: {got:?}, not the Theme style's THEMEVAL()"));
            }
        }
    }
    assert!(wrong.is_empty(), "{} problems in {path}:\n{}", wrong.len(), wrong.join("\n"));
}

/// Every connector master dropped on page 1 of a drawing whose page has a theme. → the session and each instance with its master.
fn themed_connectors(theme_first: bool) -> (Session, Vec<(&'static str, u32, u32)>) {
    let mut s = blank_session();
    if theme_first {
        run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    }
    let dropped = drop_all(&mut s);
    if !theme_first {
        run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    }
    let instances = masters_of(doc(&s), &dropped);
    (s, instances)
}

#[test]
fn every_connector_master_on_a_themed_page_saved_by_the_model_writer_takes_the_theme_through_its_master() {
    for theme_first in [true, false] {
        let (mut s, instances) = themed_connectors(theme_first);
        let dir = Dir::new();
        let path = dir.path("new.vsdx");
        run(&mut s, "file.save_as", json!({ "path": path }));
        assert_themed_through_masters(&path, &instances);
        // A second save, a splice into the file it wrote, keeps it so.
        run(&mut s, "file.save", json!({}));
        assert_themed_through_masters(&path, &instances);
    }
}

#[test]
fn a_theme_applied_to_an_opened_file_links_the_files_masters_through_the_splice() {
    // The masters are the file's: the theme changes them in their own parts.
    let dir = Dir::new();
    let base = dir.path("base.vsdx");
    let mut s = blank_session();
    let dropped = drop_all(&mut s);
    run(&mut s, "file.save_as", json!({ "path": base }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": base }));
    run(&mut t, "theme.apply", json!({ "theme": "cinderglow" }));
    let instances = masters_of(doc(&t), &dropped);
    run(&mut t, "file.save", json!({}));
    assert_themed_through_masters(&base, &instances);
}

#[test]
fn every_connector_master_dropped_on_a_themed_page_of_an_opened_file_takes_the_theme_through_the_splice() {
    let dir = Dir::new();
    let base = dir.path("base.vsdx");
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({ "master": "process", "page": 1, "x": 9.0, "y": 9.0 }));
    run(&mut s, "file.save_as", json!({ "path": base }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": base }));
    run(&mut t, "theme.apply", json!({ "theme": "inkstone" }));
    let dropped = drop_all(&mut t);
    let instances = masters_of(doc(&t), &dropped);
    run(&mut t, "file.save", json!({}));
    assert_themed_through_masters(&base, &instances);
}

#[test]
fn every_connector_master_on_a_themed_page_saved_from_a_diacraft_drawing_takes_the_theme_through_its_master() {
    let (mut s, instances) = themed_connectors(true);
    let dir = Dir::new();
    let native = dir.path("drawing.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": native }));
    let path = dir.path("from-native.vsdx");
    run(&mut t, "file.save_as", json!({ "path": path }));
    assert_themed_through_masters(&path, &instances);
}

#[test]
fn every_connector_master_on_a_themed_page_saved_from_an_odg_drawing_takes_the_theme_through_its_master() {
    let (mut s, made) = themed_connectors(true);
    let dropped: Vec<(&'static str, u32)> = made.iter().map(|&(name, id, _)| (name, id)).collect();
    let dir = Dir::new();
    let odg = dir.path("drawing.odg");
    run(&mut s, "file.save_as", json!({ "path": odg }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": odg }));
    let instances = masters_of(doc(&t), &dropped);
    let path = dir.path("from-odg.vsdx");
    run(&mut t, "file.save_as", json!({ "path": path }));
    assert_themed_through_masters(&path, &instances);
}

#[test]
fn the_theme_line_colour_reaches_each_connector_instance_through_its_master() {
    let (mut s, instances) = themed_connectors(true);
    let a = id_of(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 9.5, "y": 7.5 })));
    let b = id_of(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 10.5, "y": 7.5 })));
    let plain = id_of(&run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } })));
    let themed = look(&mut s, 1, plain)["line"].clone();
    assert_ne!(themed, json!("#000000"), "a connector of its own takes the theme's line colour");
    let mut reached = 0;
    for &(name, id, master) in &instances {
        let holds = doc(&s).master(master).unwrap().shapes.get(0).unwrap().sheet.cell("LineColor").is_some();
        if holds {
            continue;
        }
        assert_eq!(look(&mut s, 1, id)["line"], themed, "{name}: the theme's line colour");
        reached += 1;
    }
    assert!(reached >= 20, "{reached} of {} instances", instances.len());
}

// ---- what DiaCraft draws ---------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn every_master_of_the_catalogue_draws_on_a_themed_page_as_it_did_when_each_instance_was_linked_itself() {
    for (k, theme) in THEMES.iter().enumerate() {
        for variant in [0, 2] {
            let mut s = blank_session();
            if k == 0 && variant == 2 {
                run(&mut s, "theme.apply", json!({ "theme": theme, "variant": variant }));
                drop_every(&mut s, 1);
            } else {
                drop_every(&mut s, 1);
                run(&mut s, "theme.apply", json!({ "theme": theme, "variant": variant }));
            }
            let d = doc(&s);
            assert_through_masters(d, 1);
            assert_same_picture(d, &as_linked_on_their_own(d, &[1]), 1, &format!("{theme}, variant {variant}"));
        }
    }
}

#[test]
fn a_c4_element_takes_the_themes_accent_through_its_master() {
    let drop_two = |s: &mut Session| {
        let person = id_of(&run(s, "shape.drop", json!({ "master": "c4.person", "stencil": "c4", "page": 1, "x": 3.0, "y": 4.0 })));
        let container = id_of(&run(s, "shape.drop", json!({ "master": "c4.container", "stencil": "c4", "page": 1, "x": 7.0, "y": 4.0 })));
        (person, container)
    };
    let mut plain = blank_session();
    let (p, c) = drop_two(&mut plain);
    let unthemed = (look(&mut plain, 1, p)["fill"].clone(), look(&mut plain, 1, c)["fill"].clone());
    for theme_first in [true, false] {
        let mut s = blank_session();
        if theme_first {
            run(&mut s, "theme.apply", json!({ "theme": "plumline" }));
        }
        let (person, container) = drop_two(&mut s);
        if !theme_first {
            run(&mut s, "theme.apply", json!({ "theme": "plumline" }));
        }
        let accents =
            run(&mut s, "theme.list", json!({}))["themes"].as_array().unwrap().iter().find(|t| t["id"] == "plumline").unwrap()["colours"]["accents"]
                .clone();
        let fills = (look(&mut s, 1, person)["fill"].clone(), look(&mut s, 1, container)["fill"].clone());
        assert!(accents.as_array().unwrap().contains(&fills.1), "the container's fill {} is an accent of the theme {accents}", fills.1);
        assert_ne!(fills, unthemed, "the theme's colours, not those of no theme");
        let d = doc(&s);
        assert_through_masters(d, 1);
        assert_same_picture(d, &as_linked_on_their_own(d, &[1]), 1, &format!("theme first: {theme_first}"));
    }
}

/// A few masters of several kinds, dropped on page `page`: two-dimensional ones, a connector master, a C4 element and a group master's members.
fn a_few(s: &mut Session, page: u32) -> Vec<u32> {
    [
        ("basic", "rectangle"),
        ("flowchart", "process"),
        ("flowchart", "decision"),
        ("uml-class", "uml-class.generalization"),
        ("bpmn", "bpmn.sequence-flow"),
        ("c4", "c4.person"),
    ]
    .iter()
    .enumerate()
    .map(|(k, (stencil, master))| {
        id_of(&run(s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": page, "x": 1.5 + k as f64 * 1.5, "y": 4.0 })))
    })
    .collect()
}

#[test]
fn one_master_serves_pages_of_different_themes_and_a_page_with_none() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({}));
    run(&mut s, "page.add", json!({}));
    let pages: Vec<u32> = doc(&s).pages.iter().map(|p| p.id).collect();
    let [one, two, three] = [pages[0], pages[1], pages[2]];
    for page in [one, two, three] {
        a_few(&mut s, page);
    }
    let plain_three = drawn(doc(&s), three);
    let masters = doc(&s).masters.len();
    run(&mut s, "theme.apply", json!({ "theme": "tidepool", "page": one }));
    run(&mut s, "theme.apply", json!({ "theme": "cinderglow", "variant": 2, "page": two }));
    let d = doc(&s).clone();
    assert_eq!(d.masters.len(), masters, "no master was copied");
    assert_through_masters(&d, one);
    assert_through_masters(&d, two);
    for s3 in standing(&d, three) {
        assert_eq!(s3.own, Links::default(), "a shape of the page with no theme is left as it was");
    }
    let old = as_linked_on_their_own(&d, &[one, two]);
    for page in [one, two, three] {
        assert_same_picture(&d, &old, page, &format!("page {page}"));
    }
    assert_eq!(drawn(&d, three), plain_three, "the page with no theme draws as it did before any theme");
    // Each page shows its own theme: the same master's line is two colours.
    let lines: Vec<Json> = [one, two].iter().map(|p| look(&mut s, *p, standing(&d, *p)[3].id)["line"].clone()).collect();
    assert_ne!(lines[0], lines[1], "{lines:?}");
    // A variant of the second page, and no theme on the first.
    run(&mut s, "theme.variant", json!({ "variant": 1, "page": two }));
    run(&mut s, "theme.apply", json!({ "theme": "none", "page": one }));
    let d = doc(&s).clone();
    let old = as_linked_on_their_own(&d, &[one, two]);
    for page in [one, two, three] {
        assert_same_picture(&d, &old, page, &format!("page {page} after a variant and no theme"));
    }
    assert_eq!(drawn(&d, three), plain_three);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_instance_on_a_locked_layer_keeps_its_look_and_the_others_of_its_master_are_linked_on_their_own() {
    let mut s = blank_session();
    let locked = id_of(&run(&mut s, "shape.drop", json!({ "master": "process", "page": 1, "x": 2.0, "y": 3.0 })));
    let free = id_of(&run(&mut s, "shape.drop", json!({ "master": "process", "page": 1, "x": 4.0, "y": 3.0 })));
    let other = id_of(&run(&mut s, "shape.drop", json!({ "master": "uml-class.generalization", "page": 1, "x": 6.0, "y": 3.0 })));
    run(&mut s, "layer.add", json!({ "name": "Frozen" }));
    run(&mut s, "layer.assign", json!({ "ids": [locked], "layers": ["Frozen"] }));
    run(&mut s, "layer.set", json!({ "layer": "Frozen", "lock": true }));
    let before = doc(&s).clone();
    let out = run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    assert_eq!(out["locked"], json!(1), "{out}");
    let d = doc(&s).clone();
    let theme = Some(theme_id(&d));
    let by_id = |id: u32| standing(&d, 1).into_iter().find(|x| x.id == id).unwrap();
    // The master the locked shape stands for is not linked (that would change the locked shape's look), so the other instance of it links itself, as before.
    assert_eq!(by_id(locked).own, Links::default(), "nothing is written into the locked shape");
    assert_eq!(by_id(locked).theirs, before.master(by_id(locked).master).unwrap().shapes.get(0).unwrap().sheet.links, "its master is as it was");
    assert_eq!(by_id(free).own, Links { line: theme, fill: theme, text: theme });
    // A master no locked shape stands for is linked.
    assert_eq!(by_id(other).own, Links::default());
    assert_eq!(by_id(other).theirs, Links { line: theme, fill: theme, text: theme });
    assert_same_picture(&d, &as_linked_on_their_own(&d, &[1]), 1, "the locked layer");
    // The locked shape draws as it did before the theme: it follows no style.
    let alone = |d: &Document| {
        let mut only = d.clone();
        let p = Arc::make_mut(only.pages.get_mut(0).unwrap());
        p.shapes = ShapeList::from_vec(p.shapes.iter().filter(|x| x.id == locked).cloned().collect());
        only
    };
    assert_same_picture(&alone(&d), &alone(&before), 1, "the locked shape alone");
}

#[test]
fn a_shape_of_another_themed_page_that_does_not_follow_the_theme_keeps_its_master_from_being_linked() {
    // Page 2 takes a theme while its shape is on a locked layer, so it does not follow it; unlocked, it still follows nothing. A theme on page 1 must not reach it through the master.
    let mut s = blank_session();
    run(&mut s, "page.add", json!({}));
    let two = doc(&s).pages.get(1).unwrap().id;
    let mine = id_of(&run(&mut s, "shape.drop", json!({ "master": "decision", "page": 1, "x": 2.0, "y": 3.0 })));
    let theirs = id_of(&run(&mut s, "shape.drop", json!({ "master": "decision", "page": two, "x": 2.0, "y": 3.0 })));
    run(&mut s, "layer.add", json!({ "name": "Frozen", "page": two }));
    run(&mut s, "layer.assign", json!({ "ids": [theirs], "layers": ["Frozen"], "page": two }));
    run(&mut s, "layer.set", json!({ "layer": "Frozen", "lock": true, "page": two }));
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "page": two }));
    run(&mut s, "layer.set", json!({ "layer": "Frozen", "lock": false, "page": two }));
    let page_two = drawn(doc(&s), two);
    run(&mut s, "theme.apply", json!({ "theme": "tidepool", "page": 1 }));
    let d = doc(&s).clone();
    let theme = Some(theme_id(&d));
    assert_eq!(drawn(&d, two), page_two, "the other page draws as it did");
    let mine = standing(&d, 1).into_iter().find(|x| x.id == mine).unwrap();
    assert_eq!(mine.own, Links { line: theme, fill: theme, text: theme }, "the shape links itself, as before");
    assert_eq!(mine.theirs.line, Some(0), "and its master is left on the root");
}

// ---- undo, redo and saving what is unchanged -------------------------------------------------------------------------------------------------------------------------------

#[test]
fn undo_and_redo_of_a_theme_and_of_a_variant_put_the_masters_back_and_forward() {
    let mut s = blank_session();
    a_few(&mut s, 1);
    let before = doc(&s).clone();
    run(&mut s, "theme.apply", json!({ "theme": "tidepool" }));
    let themed = doc(&s).clone();
    assert_through_masters(&themed, 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(*doc(&s) == before, "undo puts the masters' links back");
    run(&mut s, "edit.redo", json!({}));
    assert!(*doc(&s) == themed, "redo links them again");
    run(&mut s, "theme.variant", json!({ "variant": 3 }));
    assert_through_masters(doc(&s), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(*doc(&s) == themed);
    // No theme: the masters stay linked, and the page draws as the root style does.
    run(&mut s, "theme.apply", json!({ "theme": "none" }));
    let none = doc(&s).clone();
    assert_through_masters(&none, 1);
    assert_same_picture(&none, &before, 1, "no theme");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_unedited_save_of_a_themed_file_is_the_same_bytes_and_an_earlier_files_instances_are_left_as_they_are_until_a_theme_is_applied() {
    let dir = Dir::new();
    // A file of this build.
    let (mut s, _) = themed_connectors(true);
    let path = dir.path("now.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let bytes = std::fs::read(&path).unwrap();
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": path }));
    run(&mut t, "file.save", json!({}));
    assert!(std::fs::read(&path).unwrap() == bytes, "an unedited save of a file of this build is the same bytes");
    // A file of an earlier build: each instance linked to the Theme style itself.
    let (s, _) = themed_connectors(true);
    let earlier = as_linked_on_their_own(doc(&s), &[1]);
    let mut e = blank_session();
    e.edit("an earlier build's drawing", |d| {
        *d = earlier.clone();
        Ok(())
    })
    .unwrap();
    let old = dir.path("earlier.vsdx");
    run(&mut e, "file.save_as", json!({ "path": old }));
    let bytes = std::fs::read(&old).unwrap();
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": old }));
    let theme = Some(theme_id(doc(&t)));
    for x in standing(doc(&t), 1) {
        assert_eq!(x.own, Links { line: theme, fill: theme, text: theme }, "the file's instances are as the file has them");
    }
    run(&mut t, "file.save", json!({}));
    assert!(std::fs::read(&old).unwrap() == bytes, "and an unedited save is the same bytes");
    // A theme applied in DiaCraft moves the links to the masters.
    run(&mut t, "theme.apply", json!({ "theme": "fernwood" }));
    assert_through_masters(doc(&t), 1);
    let instances: Vec<(&'static str, u32, u32)> = standing(doc(&t), 1).iter().map(|x| ("an instance", x.id, x.master)).collect();
    run(&mut t, "file.save", json!({}));
    assert_themed_through_masters(&old, &instances);
}
