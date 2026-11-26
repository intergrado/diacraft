//! The law of the fuzz target `mxfile_read`: bytes become a draw.io file, or the cells of a page, or a recipe of cells that collide on purpose (ids, parents, flags,
//! geometry), and the reader of the file, the pages and the cells is held to its contract on them, with the real limits and with small ones that reach each cap.
//!
//! **The input.** The first byte picks what the rest is: 0, the bytes as a file; 1, the bytes as what stands inside the `root` of a plain page; 2, a recipe (four bytes make a cell:
//! its id and its parent out of a handful of ids, so that parents repeat, cycle and dangle, its flags, and a geometry out of a handful of numbers that includes the bad ones); 3, the
//! same recipe in a page that is compressed, behind a model whose settings the bytes choose.
//!
//! **The properties.** The reader never panics and never hangs; the work it counts never passes the budget, whether it reads or refuses; what it gives is the same the second time;
//! the drawing it gives holds the model's caps; and every graph of cells it gives holds [`tree_holds`]: one root, layers that are the root's children, every cell reached from the
//! root once and nothing else (the others left out, and no list of children holds them), depths that are the parent's plus one and within the cap, ids that are unique and found,
//! roles that agree with the flags and the geometry, and every number finite and within the cap.

use std::collections::HashSet;

use diacraft_mxfile::cells::{Graph, Link, Role};
use diacraft_mxfile::file::{self, PageSource};
use diacraft_mxfile::{Limits, ReadWork, Reading, read_counted};

/// Whether `v` is a number a length may be within `max`.
fn within(v: f64, max: f64) -> bool {
    v.is_finite() && v.abs() <= max
}

/// The roles that a cell with a box has: what `rect` is for.
fn has_box(role: Role) -> bool {
    matches!(role, Role::Shape | Role::Port | Role::Neither)
}

/// Whether every number of one cell is finite and within the cap, its box has a size that is not below 0 and its geometry holds the cap of points.
fn numbers_hold(c: &diacraft_mxfile::cells::Cell, limits: &Limits) -> Result<(), String> {
    let max = limits.max_coord_px;
    if !within(c.frame.x, max) || !within(c.frame.y, max) {
        return Err(format!("the frame of {:?} is {:?}", c.id, c.frame));
    }
    if let Some(r) = c.rect
        && (![r.x, r.y, r.width, r.height].iter().all(|v| within(*v, max)) || r.width < 0.0 || r.height < 0.0)
    {
        return Err(format!("the box of {:?} is {r:?}", c.id));
    }
    if let Some(g) = c.geometry.as_deref() {
        let nums = [g.x, g.y, g.width, g.height, g.offset.x, g.offset.y];
        if !nums.iter().all(|v| within(*v, max)) || g.width < 0.0 || g.height < 0.0 {
            return Err(format!("the geometry of {:?} is {g:?}", c.id));
        }
        if g.points.len() > limits.max_points {
            return Err(format!("{} waypoints of {:?}", g.points.len(), c.id));
        }
        let pts = g.points.iter().chain(g.source_point.iter()).chain(g.target_point.iter());
        if !pts.into_iter().all(|p| within(p.x, max) && within(p.y, max)) {
            return Err(format!("a point of {:?} is past the cap", c.id));
        }
    }
    Ok(())
}

/// Whether the role of a cell agrees with its flags, its geometry and its parent.
fn role_holds(g: &Graph, id: diacraft_mxfile::cells::CellId, root: diacraft_mxfile::cells::CellId) -> Result<(), String> {
    let Some(c) = g.cell(id) else { return Err("a cell that is not there".to_owned()) };
    let relative = c.geometry.as_deref().is_some_and(|geo| geo.relative);
    let parent_role = c.parent.and_then(|p| g.cell(p)).map(|p| p.role);
    let layer = c.parent == Some(root);
    let ok = match c.role {
        Role::Root | Role::Dropped => true,
        Role::Layer => layer && c.rect.is_none(),
        Role::Connector => !layer && c.edge && c.rect.is_none(),
        Role::Label => !layer && c.vertex && !c.edge && parent_role == Some(Role::Connector) && relative && c.rect.is_none(),
        Role::Port => {
            !layer
                && c.vertex
                && !c.edge
                && matches!(parent_role, Some(Role::Shape | Role::Port | Role::Label))
                && relative
                && c.rect.is_some() == c.geometry.is_some()
        }
        Role::Shape => {
            !layer
                && c.vertex
                && !c.edge
                && !(relative && matches!(parent_role, Some(Role::Shape | Role::Port | Role::Label | Role::Connector)))
                && c.rect.is_some() == c.geometry.is_some()
        }
        Role::Neither => !layer && !c.vertex && !c.edge && c.rect.is_some() == c.geometry.is_some(),
    };
    if !ok {
        return Err(format!("the cell {:?} is a {:?} that its flags, geometry or parent do not make one", c.id, c.role));
    }
    debug_assert!(has_box(c.role) || c.rect.is_none());
    for end in [c.source, c.target] {
        if let Link::Cell(to) = end
            && g.cell(to).is_none_or(|t| t.role == Role::Dropped)
        {
            return Err(format!("an end of {:?} names a cell that is not there", c.id));
        }
    }
    Ok(())
}

/// The invariants of a graph of cells; the reason when it breaks one. Linear in the cells.
///
/// # Errors
/// The first invariant that does not hold, in words.
pub fn tree_holds(g: &Graph, limits: &Limits) -> Result<(), String> {
    let n = g.cells.len();
    if n > limits.max_shapes_per_page {
        return Err(format!("{n} cells, past the cap"));
    }
    let Some(root) = g.root else {
        return if n == 0 { Ok(()) } else { Err("cells and no root".to_owned()) };
    };
    let mut seen = vec![false; n];
    let mut ids: HashSet<&str> = HashSet::new();
    let mut reached = 0usize;
    for (id, c) in g.iter() {
        match c.role {
            Role::Root => {
                if id != root || c.parent.is_some() || c.depth != 0 {
                    return Err(format!("the root {:?} is not the root", c.id));
                }
            }
            Role::Dropped => {
                if c.parent.is_some() || c.depth != 0 || !g.children(id).is_empty() {
                    return Err(format!("the left-out cell {:?} is in the tree", c.id));
                }
            }
            _ => {
                reached += 1;
                let p = c.parent.ok_or_else(|| format!("the cell {:?} has no parent", c.id))?;
                let pc = g.cell(p).ok_or("a parent that is not there")?;
                if pc.role == Role::Dropped || c.depth != pc.depth.saturating_add(1) || c.depth as usize > limits.max_cell_depth.saturating_add(1) {
                    return Err(format!("the depth of {:?} is {} under a parent at {}", c.id, c.depth, pc.depth));
                }
            }
        }
        role_holds(g, id, root)?;
        numbers_hold(c, limits)?;
        if !c.id.is_empty() {
            if !ids.insert(c.id.as_str()) {
                return Err(format!("the id {:?} is two cells'", c.id));
            }
            if g.by_id(&c.id) != Some(id) {
                return Err(format!("the id {:?} does not find its cell", c.id));
            }
        }
    }
    let mut listed = 0usize;
    for (id, _) in g.iter() {
        for child in g.children(id) {
            let slot = seen.get_mut(child.index()).ok_or("a child that is not there")?;
            if *slot {
                return Err("a cell in two lists of children".to_owned());
            }
            *slot = true;
            listed += 1;
            if g.cell(*child).and_then(|c| c.parent) != Some(id) {
                return Err("a child whose parent is another".to_owned());
            }
        }
    }
    if listed != reached {
        return Err(format!("{listed} cells in the lists of children and {reached} that are below the root"));
    }
    if g.layers().iter().any(|l| g.cell(*l).is_none_or(|c| c.role != Role::Layer)) {
        return Err("a child of the root that is no layer".to_owned());
    }
    Ok(())
}

/// The ids a recipe picks from: a repeat, a cycle and a dangling parent come from these few. The empty one is a cell with no id.
const IDS: [&str; 8] = ["0", "1", "a", "b", "c", "d", "e", ""];
/// The numbers a recipe's geometry picks from, bad ones included.
const NUMS: [&str; 12] = ["0", "10", "-5", "1e9", "nan", "0.5", "-1", "1e999", "12.5", " 7 ", "", "-1e30"];

/// The cells a recipe of `bytes` makes: four bytes a cell.
fn recipe(bytes: &[u8]) -> String {
    let mut out = String::new();
    for quad in bytes.as_chunks::<4>().0.iter().take(300) {
        let [a, b, f, n] = *quad;
        let id = IDS.get(usize::from(a) % IDS.len()).copied().unwrap_or("");
        let parent = IDS.get(usize::from(b) % (IDS.len() + 1)).copied();
        let num = |k: usize| NUMS.get((usize::from(n) + k) % NUMS.len()).copied().unwrap_or("0");
        let mut cell = String::from("<mxCell");
        if f & 128 == 0 || id.is_empty() {
            cell.push_str(&format!(" id=\"{id}\""));
        }
        if let Some(p) = parent {
            cell.push_str(&format!(" parent=\"{p}\""));
        }
        for (bit, attr) in [(1u8, " vertex=\"1\""), (2, " edge=\"1\""), (64, " visible=\"0\""), (32, " odd=\"1\"")] {
            if f & bit != 0 {
                cell.push_str(attr);
            }
        }
        if f & 4 != 0 {
            cell.push_str(" source=\"a\" target=\"b\"");
        }
        let (rel, geo) = (if f & 16 != 0 { " relative=\"1\"" } else { "" }, f & 8 != 0);
        if geo {
            cell.push_str(&format!(
                "><mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rel} as=\"geometry\"><Array as=\"points\"><mxPoint x=\"{}\" y=\"{}\"/><mxPoint/></Array><mxPoint x=\"{}\" as=\"offset\"/></mxGeometry></mxCell>",
                num(0),
                num(1),
                num(2),
                num(3),
                num(4),
                num(5),
                num(6)
            ));
        } else {
            cell.push_str("/>");
        }
        out.push_str(&cell);
    }
    out
}

/// A plain page around `cells`, with the settings of a model that the bytes `s` choose.
fn page_of(cells: &str, s: u8) -> String {
    let num = |k: u8| NUMS.get(usize::from(s.wrapping_add(k)) % NUMS.len()).copied().unwrap_or("0");
    format!(
        "<mxfile vars=\"{{&quot;a&quot;:&quot;1&quot;}}\"><diagram name=\"d\"><mxGraphModel pageWidth=\"{}\" pageHeight=\"{}\" pageScale=\"{}\" background=\"#fff\"><root>{cells}</root></mxGraphModel></diagram></mxfile>",
        num(0),
        num(3),
        num(7)
    )
}

/// The limits the second run of every input is held to: small, so that every cap is one the bytes can reach.
fn small() -> Limits {
    Limits {
        max_cell_depth: 3,
        max_points: 1,
        max_coord_px: 1000.0,
        max_page_px: 2000.0,
        max_shapes_per_page: 40,
        max_page_elements: 400,
        max_vars: 1,
        max_json_depth: 2,
        max_number_bytes: 6,
        ..Limits::default()
    }
}

/// Every page of `bytes` as the walk hands them on, within `limits`, with the work; `None` for a refusal.
fn pages(bytes: &[u8], limits: &Limits) -> (Option<(Vec<PageSource>, file::Mxfile)>, ReadWork) {
    let mut reading = Reading::new(limits);
    let mut out = Vec::new();
    let result = file::read_sources(bytes, &mut reading, |_, page| {
        out.push(page);
        Ok(())
    });
    (result.ok().map(|mx| (out, mx)), reading.report.work)
}

/// The reader on `bytes` within `limits`: the whole read and the pages of the walk each hold the contract, twice the same.
fn whole(bytes: &[u8], limits: &Limits) {
    let mut work = ReadWork::default();
    let first = read_counted(bytes, limits, &mut work);
    assert!(work.total() <= limits.max_read_work, "the work {} is past the budget", work.total());
    let mut again = ReadWork::default();
    let second = read_counted(bytes, limits, &mut again);
    assert_eq!(work, again, "the same file, two counts");
    match (first, second) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.doc, b.doc, "the same file, two drawings");
            assert_eq!(a.report, b.report, "the same file, two reports");
            assert!(a.doc.check_limits().is_ok(), "a drawing past the caps of the model");
            assert_eq!(a.doc.pages.len(), a.report.pages as usize, "a page for each page counted");
        }
        (Err(a), Err(b)) => assert_eq!((a.kind, a.class, a.words), (b.kind, b.class, b.words), "the same file, two refusals"),
        _ => panic!("the same file, a drawing and a refusal"),
    }
    let (walked, walk_work) = pages(bytes, limits);
    assert!(walk_work.total() <= limits.max_read_work, "the work of the walk is past the budget");
    let (again, again_work) = pages(bytes, limits);
    assert_eq!(walk_work, again_work, "the same file, two counts of the walk");
    assert_eq!(walked, again, "the same file, two walks");
    if let Some((pages, _)) = walked {
        for page in &pages {
            if let Err(why) = tree_holds(&page.graph, limits) {
                panic!("the cells of {:?} break an invariant: {why}", page.name);
            }
        }
    }
}

/// The contract of the reader of the file, the pages and the cells on `data`; panics when it breaks it.
pub fn check(data: &[u8]) {
    let (mode, body) = data.split_first().map_or((0, &[][..]), |(m, b)| (*m, b));
    let limits = [Limits::default(), small()];
    for limits in &limits {
        match mode % 4 {
            0 => whole(body, limits),
            1 => {
                let inner = String::from_utf8_lossy(body);
                whole(page_of(&inner, 0).as_bytes(), limits);
            }
            2 => {
                let (s, rest) = body.split_first().map_or((0, &[][..]), |(s, r)| (*s, r));
                whole(page_of(&recipe(rest), s).as_bytes(), limits);
            }
            _ => {
                let (s, rest) = body.split_first().map_or((0, &[][..]), |(s, r)| (*s, r));
                let model = format!(
                    "<mxGraphModel pageWidth=\"{}\"><root>{}</root></mxGraphModel>",
                    NUMS.get(usize::from(s) % NUMS.len()).copied().unwrap_or("0"),
                    recipe(rest)
                );
                let file =
                    format!("<mxfile><diagram name=\"p\">{}</diagram><diagram>{}</diagram></mxfile>", super::encode(&model), super::encode(&model));
                whole(file.as_bytes(), limits);
            }
        }
    }
}
