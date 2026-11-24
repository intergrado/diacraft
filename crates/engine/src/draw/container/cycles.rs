//! Our rule: the specifications say what a container holds and that a shape names its container back, and nothing of a loop. **Membership is
//! a tree**: a container is never its own member, directly or through the shapes it holds, by its members (code 1) or by the containers its shapes are in (code 4 and the edges, 7 to 10).
//! Every tie a command makes is asked first (`Graph::join`, `Graph::would_loop`), so no command makes a loop; a file can hold one, and **it is broken when the file is read, in words, with
//! nothing else changed** ([`untangle`]): the tie that closes each loop is taken out, on both sides, and every other call of the shapes' `Relationships` stays in its place.
//!
//! **Which tie goes.** The page's shapes are walked depth first in drawing order (back to front), each following the shapes it holds by either side of their ties, and a tie that leads
//! back to a shape the walk is still inside closes a loop: that one is taken out. So of two containers that hold each other, the one drawn first (behind, as a container is) keeps the
//! other, and a loop of any length loses one tie. A tie of a shape to itself goes too.
//!
//! **What it costs.** One read of the page's ties ([`Page::read`], charged to its budget as every container read is: `page::MAX_WORK` steps), a step for each tie and one walk of the
//! page's shapes, whatever the number and length of the loops: a file of a loop of ten thousand containers opens within a small multiple of the same containers in a chain. A page past
//! the budget is left as it was, and the note says it was not looked at (the walks that follow a move and the fit stay bounded with a loop: each visits a shape once).

use std::collections::{BTreeMap, HashMap, HashSet};

use diacraft_doc::{Document, MAX_SHAPES_PER_PAGE};

use super::page::Page;
use crate::draw::Problem;
use crate::draw::ties::{IN_CONTAINER, IN_LIST, LIST_MEMBERS, MEMBERS, ON_EDGE, Ties};

/// The ties a note names one by one; it counts the rest.
const NAMED: usize = 5;

/// A tie taken out to break a loop: the shape `member` is no longer a member of `container` (the same shape for a tie of a shape to itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Broken {
    pub container: u32,
    pub member: u32,
}

/// The masters that have a shape with a `Relationships` cell of its own, which their instances take: a look at each shape of each master, at most [`MAX_SHAPES_PER_PAGE`] in all.
fn masters_with_ties(doc: &Document) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut visits = MAX_SHAPES_PER_PAGE;
    for master in doc.masters.iter() {
        let mut stack: Vec<&diacraft_doc::Shape> = master.shapes.iter().map(|s| &**s).collect();
        while let Some(shape) = stack.pop() {
            if visits == 0 {
                // Past the most that is looked at, every master is taken to have one: its instances' pages are read.
                return doc.masters.iter().map(|m| m.id).collect();
            }
            visits -= 1;
            if shape.sheet.cell("Relationships").is_some() {
                out.insert(master.id);
                break;
            }
            stack.extend(shape.children.iter().map(|c| &**c));
        }
    }
    out
}

/// Whether a page has a shape whose `Relationships` could tie it to another: one of its own, or its master's. A page with none is not read.
fn may_hold_ties(page: &diacraft_doc::Page, masters: &HashSet<u32>) -> bool {
    page.shapes.iter().any(|s| s.sheet.cell("Relationships").is_some() || s.master.is_some_and(|m| masters.contains(&m)))
}

/// Breaks every loop of containers on every page of `doc`, as a file is read. → the notes, in words: one for each page that had a loop, and one for a page that was
/// past the budget and was not looked at. A document with no tie costs a look at each shape's cells.
pub(crate) fn untangle(doc: &mut Document) -> Vec<String> {
    let masters = masters_with_ties(doc);
    let pages: Vec<u32> = doc.pages.iter().filter(|p| may_hold_ties(p, &masters)).map(|p| p.id).collect();
    let mut notes = Vec::new();
    for page in pages {
        match untangle_page(doc, page, None) {
            Ok(broken) if broken.is_empty() => {}
            Ok(broken) => notes.push(loop_words(page, &broken)),
            Err(problem) => notes.push(format!("the containers of page {page} were not looked at for a loop: {}", problem.message)),
        }
    }
    notes
}

/// The words of the ties taken out of the loops of page `page`.
pub(crate) fn loop_words(page: u32, broken: &[Broken]) -> String {
    let n = broken.len();
    let mut named: Vec<String> = broken
        .iter()
        .take(NAMED)
        .map(|b| {
            if b.container == b.member {
                format!("shape {} no longer names itself as its own member or container", b.member)
            } else {
                format!("shape {} is no longer a member of container {}", b.member, b.container)
            }
        })
        .collect();
    if n > NAMED {
        named.push(format!("and {} more", n - NAMED));
    }
    format!(
        "the containers of page {page} made a loop, a container held through the shapes it holds: {n} tie{} that closed it {} taken out, and nothing else was changed ({})",
        if n == 1 { "" } else { "s" },
        if n == 1 { "was" } else { "were" },
        named.join("; ")
    )
}

/// The shapes a page's ties hold, by their place in its list: what each holds by either side of the ties (it lists them under 1, or they name it under 4 or an edge), and the ties
/// themselves of the shapes that have a list of them.
struct Held {
    /// The ties of each shape that has a list of them, by its place.
    own: BTreeMap<usize, Ties>,
    /// For each place, the places of the shapes it holds, each once, in drawing order.
    holds: HashMap<usize, Vec<usize>>,
    /// The places of the shapes that tie themselves to themselves.
    selfish: Vec<usize>,
}

/// Reads the ties of the page's own shapes (only those of `only`, when given) and what each holds by them. A step of the page's budget for each tie.
fn read_ties(doc: &Document, p: &Page<'_>, page: u32, only: Option<&HashSet<u32>>) -> Result<Held, Problem> {
    let mut own: BTreeMap<usize, Ties> = BTreeMap::new();
    let mut ids: HashMap<usize, u32> = HashMap::new();
    let Some(list) = doc.page(page) else { return Ok(Held { own, holds: HashMap::new(), selfish: Vec::new() }) };
    for (at, shape) in list.shapes.iter().enumerate() {
        p.work.charge(1)?;
        if p.at(shape.id) != Some(at) || only.is_some_and(|o| !o.contains(&shape.id)) {
            continue;
        }
        if shape.master.is_none() && shape.sheet.cell("Relationships").is_none() {
            continue;
        }
        if let Some(ties) = p.plain_ties(shape.id)?
            && !ties.calls.is_empty()
        {
            own.insert(at, ties);
            ids.insert(at, shape.id);
        }
    }
    let place = |id: u32| p.at(id).filter(|_| only.is_none_or(|o| o.contains(&id)));
    let mut holds: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut selfish: Vec<usize> = Vec::new();
    for (at, ties) in &own {
        let me = ids.get(at).copied().unwrap_or(u32::MAX);
        let mut own_self = false;
        // A list holds its members too: a lane that holds its own list is a loop as a container that holds itself is.
        for m in ties.ids(MEMBERS).into_iter().chain(ties.ids(LIST_MEMBERS)) {
            p.work.charge(1)?;
            if m == me {
                own_self = true;
            } else if let Some(k) = place(m) {
                holds.entry(*at).or_default().push(k);
            }
        }
        for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE).chain([IN_LIST]) {
            for c in ties.ids(code) {
                p.work.charge(1)?;
                if c == me {
                    own_self = true;
                } else if let Some(k) = place(c) {
                    holds.entry(k).or_default().push(*at);
                }
            }
        }
        if own_self {
            selfish.push(*at);
        }
    }
    for list in holds.values_mut() {
        list.sort_unstable();
        list.dedup();
    }
    Ok(Held { own, holds, selfish })
}

/// The ties that close a loop, as (the place of the shape that holds, the place of the one it holds): one walk, depth first, from each shape in drawing order, and a step for each tie
/// followed.
fn closing(p: &Page<'_>, read: &Held) -> Result<Vec<(usize, usize)>, Problem> {
    // 1 on the way down, 2 done.
    let mut state: HashMap<usize, u8> = HashMap::with_capacity(read.holds.len());
    let mut out: Vec<(usize, usize)> = Vec::new();
    let empty: Vec<usize> = Vec::new();
    let mut roots: Vec<usize> = read.holds.keys().copied().collect();
    roots.sort_unstable();
    for root in roots {
        if state.contains_key(&root) {
            continue;
        }
        state.insert(root, 1);
        let mut frames: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(frame) = frames.last_mut() {
            let (at, next) = *frame;
            let children = read.holds.get(&at).unwrap_or(&empty);
            let Some(child) = children.get(next).copied() else {
                state.insert(at, 2);
                frames.pop();
                continue;
            };
            frame.1 = next + 1;
            p.work.charge(1)?;
            match state.get(&child) {
                None => {
                    state.insert(child, 1);
                    frames.push((child, 0));
                }
                Some(1) => out.push((at, child)),
                Some(_) => {}
            }
        }
    }
    Ok(out)
}

/// Breaks the loops of containers on page `page`, among the shapes `only` when given (the copies a paste made): the tie that closes each is taken out on both sides and the shapes'
/// `Relationships` are written again, every other call in its place. → the ties taken out, in the order the walk met them.
pub(crate) fn untangle_page(doc: &mut Document, page: u32, only: Option<&HashSet<u32>>) -> Result<Vec<Broken>, Problem> {
    let (hits, broken) = {
        let p = Page::read(doc, page)?;
        let mut read = read_ties(doc, &p, page, only)?;
        let loops = closing(&p, &read)?;
        if loops.is_empty() && read.selfish.is_empty() {
            return Ok(Vec::new());
        }
        let id_at = |at: usize| doc.page(page).and_then(|l| l.shapes.get(at)).map_or(0, |s| s.id);
        let mut changed: HashSet<usize> = HashSet::new();
        let mut broken: Vec<Broken> = Vec::new();
        for at in std::mem::take(&mut read.selfish) {
            let me = id_at(at);
            if let Some(ties) = read.own.get_mut(&at) {
                ties.remove(MEMBERS, me);
                ties.remove(LIST_MEMBERS, me);
                for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE).chain([IN_LIST]) {
                    ties.remove(code, me);
                }
                changed.insert(at);
                broken.push(Broken { container: me, member: me });
            }
        }
        for (holder, held) in loops {
            let (container, member) = (id_at(holder), id_at(held));
            // The tie is on either side or both: the container's list of members, and the member's name for its container.
            if let Some(ties) = read.own.get_mut(&holder) {
                let (a, b) = (ties.remove(MEMBERS, member), ties.remove(LIST_MEMBERS, member));
                if a || b {
                    changed.insert(holder);
                }
            }
            if let Some(ties) = read.own.get_mut(&held) {
                let mut any = false;
                for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE).chain([IN_LIST]) {
                    any |= ties.remove(code, container);
                }
                if any {
                    changed.insert(held);
                }
            }
            broken.push(Broken { container, member });
        }
        let hits: Vec<(usize, Ties)> = read.own.into_iter().filter(|(at, _)| changed.contains(at)).collect();
        (hits, broken)
    };
    super::write_back(doc, page, hits)?;
    Ok(broken)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Shape, ShapeType, Sheet, Value};
    use diacraft_testkit::fixtures::blank_document;

    use super::*;
    use crate::draw::container::page::{EditWork, Work};
    use crate::draw::page_mut;
    use crate::draw::ties::{Mark, TIES_READ};

    /// A document whose first page holds a shape for each formula, `ID`s 1 up, side by side; an empty formula is no `Relationships` cell.
    fn page_of(formulas: &[String]) -> Document {
        let mut doc = blank_document();
        let page = page_mut(&mut doc, 1).unwrap();
        for (k, formula) in formulas.iter().enumerate() {
            let id = u32::try_from(k + 1).unwrap();
            let mut shape = Shape::new(id);
            shape.kind = ShapeType::Shape;
            let mut sheet = Sheet::default();
            for (name, v) in [("PinX", f64::from(id)), ("PinY", 1.0), ("Width", 0.5), ("Height", 0.5), ("LocPinX", 0.25), ("LocPinY", 0.25)] {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())).with_unit("IN"));
            }
            if !formula.is_empty() {
                sheet.cells.insert(CellName::new("Relationships"), Cell::with_formula(formula, Some(Value::from_text("0"))));
            }
            shape.sheet = Arc::new(sheet);
            page.shapes.push(Arc::new(shape));
        }
        doc
    }

    fn ties_of(doc: &Document, id: u32) -> String {
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
        doc.shape(&at).unwrap().sheet.cell("Relationships").and_then(|c| c.formula.text().map(str::to_owned)).unwrap_or_default()
    }

    /// A ring of `n` containers: each holds the next and names the one before.
    fn ring(n: u32) -> Vec<String> {
        (1..=n)
            .map(|id| format!("SUM(DEPENDSON(1,Sheet.{}!SheetRef()),DEPENDSON(4,Sheet.{}!SheetRef()))", id % n + 1, if id == 1 { n } else { id - 1 }))
            .collect()
    }

    #[test]
    fn of_two_containers_that_hold_each_other_the_one_drawn_first_keeps_the_other_and_every_other_call_stays() {
        let mut doc = page_of(&[
            "SUM(DEPENDSON(1,Sheet.2!SheetRef(),Sheet.3!SheetRef()),DEPENDSON(4,Sheet.2!SheetRef()),DEPENDSON(3,Sheet.9!SheetRef()))".into(),
            "SUM(DEPENDSON(1,Sheet.1!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))".into(),
            "SUM(DEPENDSON(4,Sheet.1!SheetRef()))".into(),
        ]);
        let notes = untangle(&mut doc);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].contains("1 tie that closed it was taken out") && notes[0].contains("shape 1 is no longer a member of container 2"),
            "{}",
            notes[0]
        );
        assert_eq!(ties_of(&doc, 1), "SUM(DEPENDSON(1,Sheet.2!SheetRef(),Sheet.3!SheetRef()),DEPENDSON(3,Sheet.9!SheetRef()))");
        assert_eq!(ties_of(&doc, 2), "SUM(DEPENDSON(1),DEPENDSON(4,Sheet.1!SheetRef()))");
        assert_eq!(ties_of(&doc, 3), "SUM(DEPENDSON(4,Sheet.1!SheetRef()))", "a shape of no loop is not written");
        assert!(untangle(&mut doc).is_empty(), "a tree has nothing to break");
    }

    #[test]
    fn a_loop_held_by_one_side_of_its_ties_a_tie_to_itself_and_a_loop_through_a_shape_with_no_ties_are_broken() {
        // 1 lists 2 and names 2 as its container: one side each way. 3 names itself. 4 lists 5, 6 names 5 as its container (5 has no cell), and 6 lists 4.
        let mut doc = page_of(&[
            "SUM(DEPENDSON(1,Sheet.2!SheetRef()),DEPENDSON(4,Sheet.2!SheetRef()))".into(),
            "SUM(DEPENDSON(1))".into(),
            "SUM(DEPENDSON(1,Sheet.3!SheetRef()))".into(),
            "SUM(DEPENDSON(1,Sheet.5!SheetRef()))".into(),
            String::new(),
            "SUM(DEPENDSON(1,Sheet.4!SheetRef()),DEPENDSON(4,Sheet.5!SheetRef()))".into(),
        ]);
        let broken = untangle_page(&mut doc, 1, None).unwrap();
        assert_eq!(broken.len(), 3, "{broken:?}");
        assert!(broken.contains(&Broken { container: 3, member: 3 }));
        assert_eq!(ties_of(&doc, 1), "SUM(DEPENDSON(1,Sheet.2!SheetRef()))");
        assert_eq!(ties_of(&doc, 3), "SUM(DEPENDSON(1))");
        // The walk goes 4, 5, 6 and back to 4: 6's list loses 4.
        assert!(broken.contains(&Broken { container: 6, member: 4 }), "{broken:?}");
        assert_eq!(ties_of(&doc, 6), "SUM(DEPENDSON(1),DEPENDSON(4,Sheet.5!SheetRef()))");
        assert_eq!(ties_of(&doc, 4), "SUM(DEPENDSON(1,Sheet.5!SheetRef()))");
        assert!(untangle_page(&mut doc, 1, None).unwrap().is_empty());
    }

    #[test]
    fn a_list_that_holds_a_lane_that_holds_the_list_and_a_list_that_holds_itself_are_loops_and_are_broken_on_both_sides_of_their_ties() {
        // A list holds its members under 2 and each names its list under 5; a loop through them is a loop as one through containers is.
        let mut doc = page_of(&[
            "SUM(DEPENDSON(2,Sheet.2!SheetRef()))".into(),
            "SUM(DEPENDSON(5,Sheet.1!SheetRef()),DEPENDSON(1,Sheet.1!SheetRef()))".into(),
            "SUM(DEPENDSON(2,Sheet.3!SheetRef(),Sheet.4!SheetRef()))".into(),
            "SUM(DEPENDSON(5,Sheet.3!SheetRef()))".into(),
        ]);
        let broken = untangle_page(&mut doc, 1, None).unwrap();
        assert_eq!(broken.len(), 2, "{broken:?}");
        assert!(broken.contains(&Broken { container: 2, member: 1 }), "the lane gives up the list it holds: {broken:?}");
        assert!(broken.contains(&Broken { container: 3, member: 3 }), "the list gives up itself: {broken:?}");
        assert_eq!(ties_of(&doc, 1), "SUM(DEPENDSON(2,Sheet.2!SheetRef()))", "the list keeps its lane");
        assert_eq!(ties_of(&doc, 2), "SUM(DEPENDSON(5,Sheet.1!SheetRef()),DEPENDSON(1))", "the lane keeps its list and holds nothing");
        assert_eq!(ties_of(&doc, 3), "SUM(DEPENDSON(2,Sheet.4!SheetRef()))");
        assert_eq!(ties_of(&doc, 4), "SUM(DEPENDSON(5,Sheet.3!SheetRef()))", "a shape of no loop is not written");
        assert!(untangle_page(&mut doc, 1, None).unwrap().is_empty(), "a tree has nothing to break");
    }

    #[test]
    fn a_loop_its_shapes_take_from_their_master_is_broken_by_cells_of_their_own() {
        // The master's shape names the shapes 1 and 2 as its members, so each of its two instances holds the other (and names itself, which is no member).
        let mut doc = page_of(&[String::new(), String::new()]);
        let mut shape = Shape::new(1);
        let mut sheet = Sheet::default();
        sheet.cells.insert(
            CellName::new("Relationships"),
            Cell::with_formula("SUM(DEPENDSON(1,Sheet.1!SheetRef(),Sheet.2!SheetRef()))", Some(Value::from_text("0"))),
        );
        shape.sheet = Arc::new(sheet);
        doc.add_master(diacraft_doc::Master {
            id: 7,
            shapes: diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape)]),
            ..diacraft_doc::Master::default()
        })
        .unwrap();
        for at in 0..2 {
            let instance = Arc::make_mut(page_mut(&mut doc, 1).unwrap().shapes.get_mut(at).unwrap());
            instance.master = Some(7);
            instance.master_shape = Some(1);
        }
        assert!(!masters_with_ties(&doc).is_empty());
        let notes = untangle(&mut doc);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("shape 1 is no longer a member of container 2"), "{}", notes[0]);
        assert_eq!(ties_of(&doc, 1), "SUM(DEPENDSON(1,Sheet.2!SheetRef()))", "the instance drawn first keeps the other, in a cell of its own");
        assert_eq!(ties_of(&doc, 2), "SUM(DEPENDSON(1))");
        assert!(untangle(&mut doc).is_empty());
    }

    #[test]
    fn a_formula_that_is_no_list_of_ties_is_not_read_as_a_loop_and_not_written() {
        let odd = "IF(1,SUM(DEPENDSON(1,Sheet.2!SheetRef())),0)".to_owned();
        let mut doc = page_of(&[odd.clone(), "SUM(DEPENDSON(1,Sheet.1!SheetRef()))".into()]);
        assert!(untangle(&mut doc).is_empty());
        assert_eq!(ties_of(&doc, 1), odd);
    }

    #[test]
    fn a_loop_of_ten_thousand_containers_loses_one_tie_in_one_walk_with_each_shapes_ties_read_once() {
        const N: u32 = 10_000;
        let mut doc = page_of(&ring(N));
        let _edit = EditWork::enter();
        TIES_READ.with(|c| c.set(0));
        let broken = untangle_page(&mut doc, 1, None).unwrap();
        let reads = TIES_READ.with(std::cell::Cell::get);
        let spent = super::super::MAX_WORK - Work::new().left();
        assert_eq!(broken, [Broken { container: N, member: 1 }]);
        assert!(
            reads <= 2 * N as usize + 2,
            "the ties of each shape read twice, by the page's read and to find the loop, whatever its length: {reads}"
        );
        assert!(spent <= 24 * N as usize, "{spent} steps for {N} containers: a page read, a step a tie and one walk");
        assert_eq!(ties_of(&doc, N), format!("SUM(DEPENDSON(1),DEPENDSON(4,Sheet.{}!SheetRef()))", N - 1));
        assert_eq!(ties_of(&doc, 1), "SUM(DEPENDSON(1,Sheet.2!SheetRef()))");
        assert_eq!(ties_of(&doc, 2), "SUM(DEPENDSON(1,Sheet.3!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))", "the rest is as it was");
    }

    #[test]
    fn a_page_past_the_budget_is_left_as_it_was_and_said_so() {
        let mut doc = page_of(&ring(50));
        let before = doc.clone();
        let _edit = EditWork::enter_with(60);
        let notes = untangle(&mut doc);
        assert_eq!(doc, before);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("were not looked at for a loop") && notes[0].contains("steps of work"), "{}", notes[0]);
    }

    #[test]
    fn only_the_shapes_named_are_looked_at() {
        let mut doc = page_of(&ring(3));
        let only: HashSet<u32> = [1, 2].into_iter().collect();
        assert!(untangle_page(&mut doc, 1, Some(&only)).unwrap().is_empty(), "the loop goes through 3, which is not among them");
        let all: HashSet<u32> = [1, 2, 3].into_iter().collect();
        assert_eq!(untangle_page(&mut doc, 1, Some(&all)).unwrap(), [Broken { container: 3, member: 1 }]);
    }

    /// A small generator of numbers (xorshift), so that the drawings it makes are the same at each run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x % n.max(1)
        }
    }

    /// The (code, shape) pairs each shape's own cell names, by its place: `None` for a formula that is no list of ties, with its text.
    fn marks(doc: &Document) -> Vec<Result<HashSet<(u32, u32)>, String>> {
        doc.page(1)
            .unwrap()
            .shapes
            .iter()
            .map(|s| {
                let ties = Ties::of_cell(s.sheet.cell("Relationships"));
                if !ties.is_plain() {
                    return Err(s.sheet.cell("Relationships").and_then(|c| c.formula.text()).unwrap_or_default().to_owned());
                }
                Ok(ties
                    .calls
                    .iter()
                    .flat_map(|(code, list)| list.iter().filter_map(move |m| if let Mark::Sheet(id) = m { Some((*code, *id)) } else { None }))
                    .collect())
            })
            .collect()
    }

    /// Whether the ties of shapes 1 to `n` make a loop, by either side: a shape holds what it lists under 1 and what names it under 4 or an edge.
    fn has_loop(marks: &[Result<HashSet<(u32, u32)>, String>], n: u32) -> bool {
        let mut holds: HashMap<u32, Vec<u32>> = HashMap::new();
        for (k, m) in marks.iter().enumerate() {
            let me = u32::try_from(k + 1).unwrap();
            for (code, other) in m.iter().flatten() {
                if !(1..=n).contains(other) {
                    continue;
                }
                match *code {
                    MEMBERS => holds.entry(me).or_default().push(*other),
                    c if c == IN_CONTAINER || ON_EDGE.contains(&c) => holds.entry(*other).or_default().push(me),
                    _ => {}
                }
            }
        }
        (1..=n).any(|start| {
            let mut seen = HashSet::new();
            let mut stack = vec![start];
            while let Some(next) = stack.pop() {
                for m in holds.get(&next).into_iter().flatten() {
                    if *m == start {
                        return true;
                    }
                    if seen.insert(*m) {
                        stack.push(*m);
                    }
                }
            }
            false
        })
    }

    #[test]
    fn drawings_of_random_ties_come_out_with_no_loop_having_lost_only_marks_and_a_tree_comes_out_as_it_went_in() {
        let mut rng = Rng(0x5eed_c0de_1234_5678);
        let mut loops = 0;
        for round in 0..600 {
            let n = 2 + u32::try_from(rng.below(12)).unwrap();
            let formulas: Vec<String> = (1..=n)
                .map(|_| match rng.below(8) {
                    0 => String::new(),
                    1 => "IF(1,SUM(DEPENDSON(1,Sheet.1!SheetRef(),Sheet.2!SheetRef())),0)".to_owned(),
                    _ => {
                        let calls: Vec<String> = (0..1 + rng.below(3))
                            .map(|_| {
                                let code = [1, 1, 4, 4, 7, 10, 3][usize::try_from(rng.below(7)).unwrap()];
                                // A shape of the page, itself, or one the page does not have.
                                let names: String =
                                    (0..rng.below(4)).map(|_| format!(",Sheet.{}!SheetRef()", 1 + rng.below(u64::from(n) + 2))).collect();
                                format!("DEPENDSON({code}{names})")
                            })
                            .collect();
                        format!("SUM({})", calls.join(","))
                    }
                })
                .collect();
            let mut doc = page_of(&formulas);
            let before = marks(&doc);
            let broken = untangle_page(&mut doc, 1, None).unwrap();
            let after = marks(&doc);
            for (k, (b, a)) in before.iter().zip(&after).enumerate() {
                match (b, a) {
                    (Ok(b), Ok(a)) => assert!(a.is_subset(b), "round {round}, shape {}: a mark was made: {formulas:?}", k + 1),
                    (b, a) => assert_eq!(b, a, "round {round}, shape {}: a formula that is no list is left as it was", k + 1),
                }
            }
            assert!(!has_loop(&after, n), "round {round}: a loop is left in {formulas:?}");
            assert!(untangle_page(&mut doc, 1, None).unwrap().is_empty(), "round {round}: a second pass breaks nothing");
            if broken.is_empty() {
                assert_eq!(before, after, "round {round}: a tree comes out as it went in");
                assert!(!has_loop(&before, n), "round {round}: a loop was missed in {formulas:?}");
            } else {
                loops += 1;
            }
        }
        assert!(loops > 50, "the drawings have loops to break: {loops}");
    }

    #[test]
    fn a_note_names_five_ties_and_counts_the_rest() {
        let broken: Vec<Broken> = (1..=8).map(|k| Broken { container: k + 100, member: k }).collect();
        let words = loop_words(4, &broken);
        assert!(words.contains("8 ties that closed it were taken out") && words.contains("and 3 more") && !words.contains("shape 6 is"), "{words}");
    }
}
