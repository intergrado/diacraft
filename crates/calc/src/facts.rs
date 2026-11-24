//! What a formula holds that an analysis wants, found once per formula ("walk a formula once by pointer").
//!
//! The oracle asks of each cell which functions its formula calls, whether it names a clock function, which cells it names, and which
//! sheets' text it reads. Done by walking the tree for each question and for each cell, that is quadratic in a formula of 100,000 nodes
//! met by 100,000 instances. A [`Facts`] is made by one walk of the tree and kept under the address of the `Formula`, which a document
//! shares between every cell that inherits it and which lives as long as the document does.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_formula::{CellRef, Expr, Formula, MAX_DEPTH, Qualifier};

use crate::eval::Evaluator;

/// A read of a shape's text that a formula makes: through `SHAPETEXT`, `TEXTWIDTH` or `TEXTHEIGHT`, or by naming `TheText`.
#[derive(Clone, Debug)]
pub(crate) enum TextRead {
    /// The formula's own sheet's text.
    Own,
    /// The text of the sheet this reference names.
    Of(CellRef),
}

/// The result of one walk of a formula.
#[derive(Debug, Default)]
pub(crate) struct Facts {
    /// The functions called, in capitals, each once, in the order met.
    pub functions: Vec<String>,
    /// Whether a clock function (`NOW`, `RAND`, `DOC*`, `FILENAME`, `DIRECTORY`) is called anywhere, in a branch taken or not.
    pub calls_clock: bool,
    /// The references with no sheet-returning call before them, which a walk of the cells a cell reads follows.
    pub refs: Vec<CellRef>,
    /// The reads of shape text.
    pub text_reads: Vec<TextRead>,
    /// Whether a function that carries a point between frames is called (`PAR`, `LOC`, `LOCTOLOC`, `LOCTOPAR`, `ANGLETOLOC`, `ANGLETOPAR`): it
    /// reads the pose cells of the shapes whose frames it carries between, which no reference names.
    pub frame: bool,
    /// Whether one of those calls reads the pose of the formula's own shape too: `LOC`, which carries a point to it, and `PAR` of a point that
    /// names no cell, which is then in its own frame. (`PAR(PNT(Sheet.4!Connections.X1, ...))` is in the frame of shape 4 and goes to its
    /// parent's: the shape's own pose is not read. The groups it is in are, in every case.)
    pub frame_own: bool,
    /// Whether the formula reads cells that its text does not name: a reference with a call before it (`LISTSHEETREF()!Width`), or a function that
    /// reads the members of a list, a container or a callout, or works a string out as a formula.
    pub dynamic: bool,
}

impl Facts {
    /// The bytes the facts hold, as accounted.
    fn size(&self) -> u64 {
        64 + (self.functions.len() as u64) * 32 + (self.refs.len() as u64) * 160 + (self.text_reads.len() as u64) * 160
    }
}

pub(crate) fn is_clock_function(name: &str) -> bool {
    matches!(name, "NOW" | "RAND" | "DOCCREATION" | "DOCLASTEDIT" | "DOCLASTPRINT" | "DOCLASTSAVE" | "FILENAME" | "DIRECTORY")
}

/// The functions that carry a point or an angle from one frame to another, reading the pose cells of both shapes.
const FRAME_FUNCTIONS: [&str; 6] = ["PAR", "LOC", "LOCTOLOC", "LOCTOPAR", "ANGLETOLOC", "ANGLETOPAR"];

/// The functions that walk a path. They read every cell of the `Geometry` section their first argument names,
/// and the `Width`, `Height` and `Rounding` of the shape that holds it, none of which the formula names: the dependency graph links them
/// (`affected.rs`), so an edit to a row, to the size or to the rounding works the cell out again and no other edit does.
pub(crate) const PATH_FUNCTIONS: [&str; 7] =
    ["ANGLEALONGPATH", "DISTTOPATH", "NEARESTPOINTONPATH", "PATHLENGTH", "PATHSEGMENT", "POINTALONGPATH", "SEGMENTCOUNT"];

/// The functions that measure the size of a shape's text. They read the character and
/// paragraph formats of the runs, the tab stops, the margins and the page's scale, none of which the formula names: the dependency graph links
/// them (`affected.rs`).
pub(crate) const TEXT_MEASURES: [&str; 2] = ["TEXTHEIGHT", "TEXTWIDTH"];

/// The functions that read cells no reference in the formula names and that the graph cannot name: sheets chosen at run time, strings read as
/// formulas, and the two that measure a shape's box, which read the geometry of the shape and of every member of a group, and their poses
/// They are worked out after every edit that changed anything; no file holds either of the box
/// functions, so no saved value is at risk of being rewritten by it.
const DYNAMIC_FUNCTIONS: [&str; 13] = [
    "EVALCELL",
    "EVALTEXT",
    "LISTSHEETREF",
    "LISTMEMBERCOUNT",
    "LISTORDER",
    "CONTAINERSHEETREF",
    "CONTAINERCOUNT",
    "CONTAINERMEMBERCOUNT",
    "CALLOUTTARGETREF",
    "CALLOUTCOUNT",
    "SETF",
    "BOUNDINGBOXDIST",
    "BOUNDINGBOXRECT",
];

/// What a formula reaches beyond the cells of its own sheet that its text names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reach {
    /// A reference with a qualifier: `Sheet.4!PinX`, `ThePage!User.Row`.
    pub qualified: bool,
    /// See [`Facts::frame`].
    pub frame: bool,
    /// See [`Facts::frame_own`].
    pub frame_own: bool,
    /// See [`Facts::dynamic`].
    pub dynamic: bool,
    /// Whether `TEXTWIDTH` or `TEXTHEIGHT` is called: it reads the page's scale, which is another sheet's.
    pub measures_text: bool,
}

impl Reach {
    /// Whether the formula reads anything beyond the unqualified cells it names.
    pub fn beyond_own_sheet(&self) -> bool {
        self.qualified || self.frame || self.dynamic || self.measures_text
    }
}

/// What a formula reaches, found by one walk that allocates nothing, so that every formula of a page can be asked. With the number of nodes met,
/// for the caller to charge.
///
/// **No allocation.** The walk recurses on the tree, which the parser keeps at most [`MAX_DEPTH`] deep (the formula crate: "code that walks a tree
/// may recurse to `MAX_DEPTH`"), so it needs no stack of its own: a formula costs the frames of its depth and no heap. A tree built by hand that
/// is deeper than that is walked from the level where it passes the depth with a stack of its own, on the heap, so that no tree is answered
/// differently or overflows the thread's stack (the tests hold this walk to the answers of one that keeps a stack from the root, for trees of
/// every depth).
pub(crate) fn reach(expr: &Expr) -> (Reach, usize) {
    let mut walk = ReachWalk::default();
    walk.visit(expr, 0);
    (walk.found, walk.nodes)
}

/// The walk of [`reach`]: what it has found, and the nodes it has met.
#[derive(Default)]
struct ReachWalk {
    found: Reach,
    nodes: usize,
}

impl ReachWalk {
    /// Meets `e` and everything under it. From [`MAX_DEPTH`] levels down the rest of the tree is walked with a stack of its own.
    fn visit(&mut self, e: &Expr, depth: usize) {
        if depth >= MAX_DEPTH {
            return self.visit_deep(e);
        }
        self.nodes += 1;
        self.note(e);
        each_child(e, &mut |child| self.visit(child, depth + 1));
    }

    /// Meets `root` and everything under it with a stack on the heap: a tree no parser made, or the levels of one that are past the recursion.
    fn visit_deep(&mut self, root: &Expr) {
        let mut stack: Vec<&Expr> = vec![root];
        while let Some(e) = stack.pop() {
            self.nodes += 1;
            self.note(e);
            each_child(e, &mut |child| stack.push(child));
        }
    }

    /// What the node `e` says of itself, and not what is under it.
    fn note(&mut self, e: &Expr) {
        let found = &mut self.found;
        match e {
            Expr::Call(c) => {
                let is = |names: &[&str]| names.iter().any(|n| c.name.eq_ignore_ascii_case(n));
                found.frame |= is(&FRAME_FUNCTIONS);
                found.dynamic |= is(&DYNAMIC_FUNCTIONS);
                found.measures_text |= is(&TEXT_MEASURES);
                if is(&["LOC"]) || (is(&["PAR"]) && !c.args.first().and_then(Option::as_ref).is_some_and(names_a_cell)) {
                    found.frame_own = true;
                }
                // A call before the call's name chooses the sheet it is read on at run time.
                found.dynamic |= c.qualifiers.iter().any(|q| matches!(q, Qualifier::Call { .. }));
            }
            Expr::Ref(r) => {
                found.qualified |= !r.qualifiers.is_empty();
                found.dynamic |= r.qualifiers.iter().any(|q| matches!(q, Qualifier::Call { .. }));
            }
            Expr::Unary(..) | Expr::Percent(_) | Expr::Binary(..) => {}
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
}

/// Calls `f` with each expression directly under `e`: the arguments of a call and of the calls in its qualifiers, the arguments of the calls in a
/// reference's qualifiers, and the operands of an operator.
fn each_child<'a>(e: &'a Expr, f: &mut impl FnMut(&'a Expr)) {
    match e {
        Expr::Call(c) => {
            c.args.iter().flatten().for_each(&mut *f);
            for q in &c.qualifiers {
                if let Qualifier::Call { args, .. } = q {
                    args.iter().flatten().for_each(&mut *f);
                }
            }
        }
        Expr::Ref(r) => {
            for q in &r.qualifiers {
                if let Qualifier::Call { args, .. } = q {
                    args.iter().flatten().for_each(&mut *f);
                }
            }
        }
        Expr::Unary(_, x) | Expr::Percent(x) => f(x),
        Expr::Binary(_, a, b) => {
            f(a);
            f(b);
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
    }
}

/// Whether an expression names a cell anywhere in its operands and call arguments (not in the arguments of a qualifier's call). By recursion to
/// [`MAX_DEPTH`], and with a stack of its own below that, as [`reach`] walks.
fn names_a_cell(expr: &Expr) -> bool {
    fn names(e: &Expr, depth: usize) -> bool {
        if depth >= MAX_DEPTH {
            return names_a_cell_by_stack(e);
        }
        match e {
            Expr::Ref(_) => true,
            Expr::Call(c) => c.args.iter().flatten().any(|a| names(a, depth + 1)),
            Expr::Unary(_, x) | Expr::Percent(x) => names(x, depth + 1),
            Expr::Binary(_, a, b) => names(a, depth + 1) || names(b, depth + 1),
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => false,
        }
    }
    names(expr, 0)
}

/// [`names_a_cell`] with a stack on the heap, for the levels of a tree past the recursion.
fn names_a_cell_by_stack(expr: &Expr) -> bool {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(_) => return true,
            Expr::Call(c) => stack.extend(c.args.iter().flatten()),
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
    false
}

fn is_text_function(name: &str) -> bool {
    matches!(name, "TEXTWIDTH" | "TEXTHEIGHT" | "SHAPETEXT")
}

fn names_the_text(r: &CellRef) -> bool {
    r.path.last().is_some_and(|s| &*s.name == "TheText")
}

/// Walks a tree once, with an explicit stack.
pub(crate) fn walk(expr: &Expr) -> Facts {
    let mut facts = Facts::default();
    // The names met so far, so that a formula of thousands of names is not searched once for each.
    let mut met: HashSet<String> = HashSet::new();
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Call(c) => {
                let upper = c.name.to_ascii_uppercase();
                if is_clock_function(&upper) {
                    facts.calls_clock = true;
                }
                if is_text_function(&upper) {
                    // The first argument names the text when it is a reference to `TheText`; anything else reads the formula's own.
                    match c.args.first() {
                        Some(Some(Expr::Ref(r))) if names_the_text(r) => facts.text_reads.push(TextRead::Of((**r).clone())),
                        _ => facts.text_reads.push(TextRead::Own),
                    }
                }
                if met.insert(upper.clone()) {
                    facts.functions.push(upper);
                }
                stack.extend(c.args.iter().flatten());
                for q in &c.qualifiers {
                    if let Qualifier::Call { args, .. } = q {
                        stack.extend(args.iter().flatten());
                    }
                }
            }
            Expr::Ref(r) => {
                if names_the_text(r) {
                    facts.text_reads.push(TextRead::Of((**r).clone()));
                }
                if r.qualifiers.iter().all(|q| matches!(q, Qualifier::Path(_))) {
                    facts.refs.push((**r).clone());
                }
                for q in &r.qualifiers {
                    if let Qualifier::Call { args, .. } = q {
                        stack.extend(args.iter().flatten());
                    }
                }
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
    let found = reach(expr).0;
    (facts.frame, facts.frame_own, facts.dynamic) = (found.frame, found.frame_own, found.dynamic);
    facts
}

impl Evaluator<'_, '_> {
    /// The facts of a formula, walked on the first ask and kept under the formula's address; `None` when the formula has no tree, or the
    /// budget or memory ran out. The walk is charged a step a node.
    pub(crate) fn facts_of(&mut self, formula: &Arc<Formula>) -> Option<Arc<Facts>> {
        let key = Arc::as_ptr(formula) as usize;
        if let Some(known) = self.ctx.facts.get(&key) {
            return Some(Arc::clone(known));
        }
        let expr = formula.ast.as_ref().ok()?;
        let nodes = expr.node_count();
        if !self.steps(nodes) {
            return None;
        }
        let facts = walk(expr);
        if !self.ctx.hold_bytes(facts.size()) {
            return None;
        }
        let facts = Arc::new(facts);
        self.ctx.facts.insert(key, Arc::clone(&facts));
        Some(facts)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn reach_of(source: &str) -> Reach {
        let formula = diacraft_formula::parse(source);
        reach(formula.ast.as_ref().unwrap()).0
    }

    #[test]
    fn a_formula_of_its_own_sheet_reaches_nothing_beyond_it() {
        for source in ["Width*0.5", "User.A+1", "Geometry1.X2*2", "IF(Width>1,Height,0)", "SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"] {
            assert!(!reach_of(source).beyond_own_sheet(), "{source}");
        }
    }

    #[test]
    fn a_qualified_reference_and_a_frame_function_and_a_reference_after_a_call_are_each_beyond_the_sheet() {
        assert!(reach_of("Sheet.4!PinX+1").qualified);
        assert!(reach_of("ThePage!PageWidth").qualified);
        let glue = reach_of("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))");
        assert!(glue.qualified && glue.frame && !glue.frame_own && !glue.dynamic, "a point in another shape's frame goes to the parent's: {glue:?}");
        let own = reach_of("PAR(PNT(1,2))");
        assert!(own.frame && own.frame_own, "a point that names no cell is in the shape's own frame: {own:?}");
        assert!(reach_of("LOC(PNT(Sheet.4!Width,0))").frame_own, "LOC carries a point to the shape's own frame");
        assert!(reach_of("loctopar(PNT(1,2),Sheet.1!PinX,Sheet.2!PinX)").frame, "in any case");
        assert!(!reach_of("LOCTOPAR(PNT(1,2),Sheet.1!PinX,Sheet.2!PinX)").frame_own, "its frames are the references'");
        assert!(reach_of("SHEETREF()!Width").dynamic, "a call before a reference chooses the sheet at run time");
        assert!(reach_of("LISTMEMBERCOUNT()").dynamic && reach_of("evaltext(\"1+1\")").dynamic);
    }

    /// A path function reads every row of the section it names, the shape's size and `Rounding`, none of which its text names. The
    /// graph links those reads, so the cell is not worked out after every edit (that rewrote saved curve lengths, which the application saved a
    /// little off). The two box functions read a shape's whole geometry and its members, which the graph cannot name: they stay dynamic.
    #[test]
    fn the_functions_that_walk_a_path_are_linked_and_not_dynamic() {
        for name in PATH_FUNCTIONS {
            let source = format!("{}(Geometry1.Path,0.5,0.5)", name.to_ascii_lowercase());
            assert!(!reach_of(&source).dynamic, "{source}");
            let walk = walk(diacraft_formula::parse(&source).ast.as_ref().unwrap());
            assert!(walk.functions.iter().any(|f| f == name), "{source}");
        }
        for source in ["BOUNDINGBOXRECT(0)", "BOUNDINGBOXDIST(0)", "1+boundingboxrect(0)"] {
            assert!(reach_of(source).dynamic, "{source}");
        }
        assert!(!reach_of("Geometry1.X2+1").dynamic, "a plain reference to a row is named, and found by its name");
        // The text measures reach the page's scale, which is another sheet's.
        assert!(reach_of("TEXTWIDTH(TheText)").measures_text && reach_of("max(1,textheight(TheText,3))").beyond_own_sheet());
        assert!(!reach_of("Width*2").measures_text);
    }

    #[test]
    fn the_flags_of_a_walk_are_the_flags_of_the_reach() {
        for source in ["PAR(PNT(Sheet.4!Connections.X1,0))", "LISTSHEETREF()!Width", "Width*2"] {
            let formula = diacraft_formula::parse(source);
            let expr = formula.ast.as_ref().unwrap();
            let (found, walked) = (reach(expr).0, walk(expr));
            assert_eq!((found.frame, found.frame_own, found.dynamic), (walked.frame, walked.frame_own, walked.dynamic), "{source}");
        }
    }

    /// The walk `reach` replaced, kept as the oracle of the tests below: a stack on the heap from the root, a `Vec` for each formula.
    fn reach_by_stack(expr: &Expr) -> (Reach, usize) {
        let mut found = Reach::default();
        let mut nodes = 0_usize;
        let mut stack: Vec<&Expr> = vec![expr];
        while let Some(e) = stack.pop() {
            nodes += 1;
            match e {
                Expr::Call(c) => {
                    let is = |names: &[&str]| names.iter().any(|n| c.name.eq_ignore_ascii_case(n));
                    found.frame |= is(&FRAME_FUNCTIONS);
                    found.dynamic |= is(&DYNAMIC_FUNCTIONS);
                    found.measures_text |= is(&TEXT_MEASURES);
                    if is(&["LOC"]) || (is(&["PAR"]) && !c.args.first().and_then(Option::as_ref).is_some_and(names_a_cell_by_stack)) {
                        found.frame_own = true;
                    }
                    stack.extend(c.args.iter().flatten());
                    for q in &c.qualifiers {
                        if let Qualifier::Call { args, .. } = q {
                            found.dynamic = true;
                            stack.extend(args.iter().flatten());
                        }
                    }
                }
                Expr::Ref(r) => {
                    found.qualified |= !r.qualifiers.is_empty();
                    for q in &r.qualifiers {
                        if let Qualifier::Call { args, .. } = q {
                            found.dynamic = true;
                            stack.extend(args.iter().flatten());
                        }
                    }
                }
                Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
                Expr::Binary(_, a, b) => {
                    stack.push(a);
                    stack.push(b);
                }
                Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
            }
        }
        (found, nodes)
    }

    fn same_walk(expr: &Expr, what: &str) {
        assert_eq!(reach(expr), reach_by_stack(expr), "{what}");
    }

    /// The formulas of a drawing and the ones the specs name, each walked by recursion and by a stack.
    #[test]
    fn the_walk_with_no_stack_gives_the_answers_of_the_walk_with_one_on_formulas_of_every_kind() {
        for source in [
            "Width*0.5",
            "Sheet.4!PinX+1",
            "ThePage!PageWidth",
            "PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))",
            "PAR(PNT(1,2))",
            "PAR(PNT(Width,2))",
            "LOC(PNT(Sheet.4!Width,0))",
            "loctopar(PNT(1,2),Sheet.1!PinX,Sheet.2!PinX)",
            "SHEETREF()!Width",
            "Sheet.5!SHEETREF()!Width",
            "LISTSHEETREF(IF(Width>1,2,3))!Height+TEXTWIDTH(TheText)",
            "IF(AND(Width>1,Height<2),MAX(Sheet.3!Width,2),-Height%)",
            "evaltext(\"1+1\")",
            "BOUNDINGBOXRECT(0)+TEXTHEIGHT(TheText,3)",
            "IF(1,,3)",
            "1+2+3+4+5+6+7+8+9+10+11+12+13+14+15+16+17+18+19+20",
            "((((((((((Width))))))))))",
            "Prop.Row_1.Value*Scratch.A1+User.B",
        ] {
            let formula = diacraft_formula::parse(source);
            match &formula.ast {
                Ok(expr) => same_walk(expr, source),
                Err(why) => panic!("{source} does not parse: {why:?}"),
            }
        }
    }

    /// Formulas made of pieces that each set a flag, nested and put together at random: the two walks answer alike.
    mod generated {
        use proptest::prelude::*;

        use super::*;

        fn leaf() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("1".to_string()),
                Just("Width".to_string()),
                Just("User.A".to_string()),
                Just("Sheet.4!PinX".to_string()),
                Just("ThePage!PageWidth".to_string()),
                Just("LISTSHEETREF()!Width".to_string()),
                Just("SHEETREF(Width)!Height".to_string()),
                Just("TheText".to_string()),
                Just("\"text\"".to_string()),
            ]
        }

        fn formula() -> impl Strategy<Value = String> {
            leaf().prop_recursive(8, 64, 4, |inner| {
                prop_oneof![
                    (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}+{b})")),
                    (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}*{b})")),
                    inner.clone().prop_map(|a| format!("-{a}")),
                    inner.clone().prop_map(|a| format!("({a})%")),
                    (
                        prop::sample::select(vec![
                            "IF",
                            "MAX",
                            "PNT",
                            "PAR",
                            "LOC",
                            "LOCTOPAR",
                            "LOCTOLOC",
                            "ANGLETOPAR",
                            "TEXTWIDTH",
                            "TEXTHEIGHT",
                            "EVALTEXT",
                            "SETF",
                            "LISTMEMBERCOUNT",
                            "BOUNDINGBOXRECT",
                            "BOUNDINGBOXDIST",
                            "par",
                            "Loc",
                        ]),
                        prop::collection::vec(prop::option::of(inner), 0..4),
                    )
                        .prop_map(|(name, args)| {
                            let args: Vec<String> = args.into_iter().map(Option::unwrap_or_default).collect();
                            format!("{name}({})", args.join(","))
                        }),
                ]
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 2_000, ..ProptestConfig::default() })]
            #[test]
            fn the_walks_answer_alike(source in formula()) {
                let parsed = diacraft_formula::parse(&source);
                if let Ok(expr) = &parsed.ast {
                    prop_assert_eq!(reach(expr), reach_by_stack(expr), "{}", source);
                }
            }
        }
    }

    fn call(name: &str, args: Vec<Option<Expr>>) -> Expr {
        Expr::Call(Box::new(diacraft_formula::Call { qualifiers: Vec::new(), name: Arc::from(name), args }))
    }

    fn reference(path: &str, sheet: Option<&str>) -> Expr {
        let segment = |name: &str| diacraft_formula::Segment { name: Arc::from(name), index: None };
        let qualifiers = sheet.map(|s| vec![Qualifier::Path(vec![segment(s)])]).unwrap_or_default();
        Expr::Ref(Box::new(diacraft_formula::CellRef { qualifiers, path: vec![segment(path)] }))
    }

    /// A tree that no parser made, deeper than the recursion goes, is walked whole: the nodes are all counted and the flags are all found, from a leaf at
    /// the bottom of a left chain, of a right chain and of calls inside calls, and with a depth that would overflow the stack of a walk that recursed
    /// to the bottom.
    #[test]
    fn a_tree_deeper_than_the_recursion_is_walked_whole_by_a_stack_of_its_own() {
        for depth in [MAX_DEPTH - 1, MAX_DEPTH, MAX_DEPTH + 1, 3 * MAX_DEPTH] {
            // A chain of sums leaning to the left, a leaf that is a qualified reference at the bottom.
            let mut left = reference("PinX", Some("Sheet.4"));
            for _ in 0..depth {
                left = Expr::Binary(diacraft_formula::BinaryOp::Add, Box::new(left), Box::new(Expr::Bool(true)));
            }
            let (found, nodes) = reach(&left);
            assert!(found.qualified && !found.frame && !found.dynamic, "left chain of {depth}: {found:?}");
            assert_eq!(nodes, 2 * depth + 1, "left chain of {depth}");
            same_walk(&left, "left chain");
            // The same leaning to the right.
            let mut right = call("SETF", vec![None]);
            for _ in 0..depth {
                right = Expr::Binary(diacraft_formula::BinaryOp::Add, Box::new(Expr::Bool(false)), Box::new(right));
            }
            let (found, nodes) = reach(&right);
            assert!(found.dynamic && !found.qualified, "right chain of {depth}: {found:?}");
            assert_eq!(nodes, 2 * depth + 1, "right chain of {depth}");
            same_walk(&right, "right chain");
            // Calls inside calls: `PAR(IF(1, PAR(... Width)))`, a `PAR` that names a cell holds no frame of its own and one that names none does.
            let mut nested = reference("Width", None);
            for _ in 0..depth {
                nested = call("PAR", vec![Some(nested)]);
            }
            let (found, nodes) = reach(&nested);
            assert!(found.frame && !found.frame_own, "calls of {depth}: {found:?}");
            assert_eq!(nodes, depth + 1, "calls of {depth}");
            same_walk(&nested, "nested calls");
            let mut nested = Expr::Bool(true);
            for _ in 0..depth {
                nested = call("PAR", vec![Some(nested)]);
            }
            assert!(reach(&nested).0.frame_own, "a PAR that names no cell reads the shape's own pose, {depth} deep");
            same_walk(&nested, "nested calls naming nothing");
        }
    }

    /// A tree a million levels deep, which no stack of a thread holds a recursion of, is answered and does not overflow (and is leaked: its `Drop` is
    /// the recursion that the walk is not).
    #[test]
    fn a_tree_a_million_levels_deep_does_not_overflow_the_stack() {
        let mut tree = reference("PinX", Some("Sheet.4"));
        for _ in 0..1_000_000 {
            tree = Expr::Unary(diacraft_formula::UnaryOp::Minus, Box::new(tree));
        }
        let (found, nodes) = reach(&tree);
        assert!(found.qualified);
        assert_eq!(nodes, 1_000_001);
        assert!(!names_a_cell(&Expr::Unary(diacraft_formula::UnaryOp::Minus, Box::new(Expr::Bool(true)))));
        assert!(names_a_cell(&tree));
        std::mem::forget(tree);
    }
}
