//! The window settings a drawing holds: whether its drawing window shows the rulers, the grid, the guides and the connection points, and
//! the page breaks.
//!
//! They are the flags of a `Window` in the VSDX windows part (`windows.xml`; they are the window's and not a page's),
//! which the model keeps whole as a tree in [`crate::VsdxHeld::windows`]. A `Window` holds `ShowRulers`, `ShowGrid`, `ShowPageBreaks`, `ShowGuides` and `ShowConnectionPoints` as children
//! (`<ShowGrid>1</ShowGrid>`), and a reader also takes them as attributes. **The drawing window** is the first `Window` marked `WindowType="Drawing"` (in any case) or
//! `WindowType="1"` (`visDrawing`, Learn's number for a drawing window), the window opens a page from. A prefix on an element does not matter: a part may bind its namespace however it likes.
//!
//! **A part that holds no drawing window holds no settings**: an empty `Windows`, or one whose windows are all of another type (a stencil's,
//! which holds no flag), is read as no part is ([`WindowFlags::held_by`] gives `None`), so the drawing shows what a new drawing shows. No other window
//! stands in for the drawing window (read the first window, a stencil's among them, as one).
//!
//! [`WindowFlags::held_by`] reads the five flags of a part, and [`WindowFlags::written`] gives the part with them set, **changing nothing else of it**: a flag that holds the value
//! already is not touched (`true` and `1` are one value), one the window holds as a child or as an attribute is changed where it is, and one it lacks goes in as a child at its
//! place in the order of the children. A part with no drawing window gains one holding the flags set; [`with_drawing_window`] puts in the window of another part (a new
//! drawing's) first, so that the flags it does not set read as that window has them. Nothing here fails, and every walk is of the part's own children: the tree is the one the
//! reader built, within its caps.

use std::sync::Arc;

use crate::document::Document;
use crate::raw::{RawAttrs, RawElement, RawNode};

/// The children of a `Window`, in the order R27 lists them (the schema's sequence): a flag that is put in goes after the last of those before it.
const ORDER: [&str; 13] = [
    "StencilGroup",
    "StencilGroupPos",
    "ShowRulers",
    "ShowGrid",
    "ShowPageBreaks",
    "ShowGuides",
    "ShowConnectionPoints",
    "GlueSettings",
    "SnapSettings",
    "SnapExtensions",
    "SnapAngles",
    "DynamicGridEnabled",
    "TabSplitterPos",
];

/// The five flags, by the name of the child that holds each.
const RULERS: &str = "ShowRulers";
const GRID: &str = "ShowGrid";
const PAGE_BREAKS: &str = "ShowPageBreaks";
const GUIDES: &str = "ShowGuides";
const POINTS: &str = "ShowConnectionPoints";

/// The flags a drawing window holds, each `None` where it holds no value (no child and no attribute, or a value that is not `0`, `1`, `true` or `false`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowFlags {
    /// `ShowRulers`: the rulers show.
    pub rulers: Option<bool>,
    /// `ShowGrid`: the grid shows.
    pub grid: Option<bool>,
    /// `ShowPageBreaks`: the page breaks show (View, Show, Page Breaks; built). The engine reads it before DiaCraft's own data, which held the box
    /// in every format before (`diacraft_engine::view::page_breaks_held`).
    pub page_breaks: Option<bool>,
    /// `ShowGuides`: the guides show.
    pub guides: Option<bool>,
    /// `ShowConnectionPoints`: the connection points of the shapes show, whatever the tool.
    pub connection_points: Option<bool>,
}

impl WindowFlags {
    /// The flags as `(name of the child, value)`, in the order.
    fn named(&self) -> [(&'static str, Option<bool>); 5] {
        [(RULERS, self.rulers), (GRID, self.grid), (PAGE_BREAKS, self.page_breaks), (GUIDES, self.guides), (POINTS, self.connection_points)]
    }

    /// The flags of the drawing window of the windows part `root` (see the module documentation): `None` for a root that is not `Windows` and for a part that **holds no drawing
    /// window**, which holds no settings; a flag the drawing window holds no value for is `None`.
    pub fn held_by(root: &RawElement) -> Option<WindowFlags> {
        if local(&root.name) != "Windows" {
            return None;
        }
        drawing_window(root).map(flags_of)
    }

    /// [`WindowFlags::held_by`], with no flag where the part holds no settings.
    pub fn of(root: &RawElement) -> WindowFlags {
        WindowFlags::held_by(root).unwrap_or_default()
    }

    /// Whether every flag is `None`: a value that sets nothing.
    pub fn is_empty(&self) -> bool {
        self.named().iter().all(|(_, v)| v.is_none())
    }

    /// These flags over `under`: each one this holds, and `under`'s where this holds none.
    pub fn over(&self, under: &WindowFlags) -> WindowFlags {
        WindowFlags {
            rulers: self.rulers.or(under.rulers),
            grid: self.grid.or(under.grid),
            page_breaks: self.page_breaks.or(under.page_breaks),
            guides: self.guides.or(under.guides),
            connection_points: self.connection_points.or(under.connection_points),
        }
    }

    /// The windows part `root` with each flag this holds set in its drawing window, or `None` when that changes nothing (every flag set holds the value already, or this
    /// sets none, or `root` is no `Windows`). A flag this holds `None` for is left as the part has it. A part with no drawing window gains one, holding the flags this sets
    /// and no other (such a part held no settings; a caller that wants the others to read as a new drawing's puts its window in first, [`with_drawing_window`]).
    /// See the module documentation for where each goes.
    pub fn written(&self, root: &RawElement) -> Option<RawElement> {
        if local(&root.name) != "Windows" || self.is_empty() {
            return None;
        }
        let mut out = root.clone();
        let at = out.children.iter().position(|n| matches!(n, RawNode::Element(e) if local(&e.name) == "Window" && is_drawing(e)));
        let changed = match at {
            Some(at) => match out.children.get_mut(at) {
                Some(RawNode::Element(window)) => self.set_in(window),
                _ => false,
            },
            None => {
                // No window is a drawing window: one is made, holding these.
                let prefix = prefix_of(&root.name);
                let id = next_id(root);
                let mut window = RawElement {
                    name: Arc::from(format!("{prefix}Window")),
                    attrs: RawAttrs::from_pairs([("ID", id.as_str()), ("WindowType", "Drawing")]),
                    children: Vec::new(),
                };
                self.set_in(&mut window);
                out.children.push(RawNode::Element(window));
                true
            }
        };
        changed.then_some(out)
    }

    /// Sets each flag this holds in `window`: → whether anything changed.
    fn set_in(&self, window: &mut RawElement) -> bool {
        let mut changed = false;
        for (name, value) in self.named() {
            let Some(value) = value else { continue };
            changed |= set_flag(window, name, value);
        }
        changed
    }
}

impl Document {
    /// The flags of the drawing window of the windows part this document holds: `None` when it holds **no settings**: no windows part as a tree (a document
    /// that came from no VSDX file and whose window settings nothing set, or one whose file's part is held as bytes), or a part with no drawing window; a flag the
    /// drawing window holds no value for is `None`.
    pub fn window_flags(&self) -> Option<WindowFlags> {
        let windows = self.vsdx.as_ref()?.windows.as_deref()?;
        WindowFlags::held_by(windows)
    }
}

/// The windows part `root` with the drawing window of the part `from` put in after its last window (a part with no drawing window holds no
/// settings, and one is made when a setting is set): the window as `from` has it, with the `ID` [`next_id`] gives for `root` and the names of it and of its child elements in
/// `root`'s prefix. `None` when `root` is no `Windows` or holds a drawing window already, or `from` holds none.
pub fn with_drawing_window(root: &RawElement, from: &RawElement) -> Option<RawElement> {
    if local(&root.name) != "Windows" || drawing_window(root).is_some() {
        return None;
    }
    let window = drawing_window(from)?;
    let prefix = prefix_of(&root.name);
    let id = next_id(root);
    let mut attrs: Vec<(&str, &str)> = vec![("ID", id.as_str())];
    attrs.extend(window.attrs.iter().filter(|(name, _)| *name != "ID"));
    let children = window
        .children
        .iter()
        .map(|node| match node {
            RawNode::Element(el) => RawNode::Element(RawElement { name: Arc::from(format!("{prefix}{}", local(&el.name))), ..el.clone() }),
            other => other.clone(),
        })
        .collect();
    let made = RawElement { name: Arc::from(format!("{prefix}Window")), attrs: RawAttrs::from_pairs(attrs), children };
    let mut out = root.clone();
    let at =
        out.children.iter().rposition(|n| matches!(n, RawNode::Element(e) if local(&e.name) == "Window")).map_or(out.children.len(), |last| last + 1);
    out.children.insert(at.min(out.children.len()), RawNode::Element(made));
    Some(out)
}

/// The part of an element's name after its prefix (`v:Window` is `Window`).
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// The prefix of a name with its colon (`v:` of `v:Window`), or nothing.
fn prefix_of(name: &str) -> &str {
    name.rsplit_once(':').map_or("", |(prefix, _)| name.get(..prefix.len() + 1).unwrap_or(""))
}

/// The `Window` children of `root`, in order.
fn windows(root: &RawElement) -> impl Iterator<Item = &RawElement> {
    root.children.iter().filter_map(|node| match node {
        RawNode::Element(el) if local(&el.name) == "Window" => Some(el),
        _ => None,
    })
}

/// Whether a window is marked as a drawing window: by the word `Drawing` (in any case), or by the number 1, which is `visDrawing` in Learn's enumeration of window types
/// (the numbers 2 and 3 are a stencil's and a sheet's, and are no drawing window).
fn is_drawing(window: &RawElement) -> bool {
    window.attrs.get("WindowType").is_some_and(|t| t.eq_ignore_ascii_case("Drawing") || t.trim() == "1")
}

/// The first window marked as a drawing window.
fn drawing_window(root: &RawElement) -> Option<&RawElement> {
    windows(root).find(|w| is_drawing(w))
}

/// The first child element of `parent` called `name` (by local name).
fn child<'a>(parent: &'a RawElement, name: &str) -> Option<&'a RawElement> {
    parent.children.iter().find_map(|node| match node {
        RawNode::Element(el) if local(&el.name) == name => Some(el),
        _ => None,
    })
}

/// The text of an element: its text children, joined.
fn text_of(el: &RawElement) -> String {
    el.children
        .iter()
        .filter_map(|node| match node {
            RawNode::Text(text) => Some(&**text),
            _ => None,
        })
        .collect()
}

/// A flag's value: `1` and `true` are on, `0` and `false` off, blanks around them aside; anything else is no value.
pub(crate) fn value_of(text: &str) -> Option<bool> {
    match text.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// The flag `name` of `window`: a child, or, where there is none, an attribute.
fn flag(window: &RawElement, name: &str) -> Option<bool> {
    match child(window, name) {
        Some(el) => value_of(&text_of(el)),
        None => value_of(window.attrs.get(name)?),
    }
}

fn flags_of(window: &RawElement) -> WindowFlags {
    WindowFlags {
        rulers: flag(window, RULERS),
        grid: flag(window, GRID),
        page_breaks: flag(window, PAGE_BREAKS),
        guides: flag(window, GUIDES),
        connection_points: flag(window, POINTS),
    }
}

/// The text a flag is written as.
fn written_value(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// Sets the flag `name` of `window` to `on`: → whether it changed. A child is given the value as its text (its attributes stay); with no child, an attribute of that name is
/// given it; with neither, a child goes in at its place in [`ORDER`], in the window's prefix.
fn set_flag(window: &mut RawElement, name: &str, on: bool) -> bool {
    if flag(window, name) == Some(on) {
        return false;
    }
    let value = written_value(on);
    if let Some(RawNode::Element(el)) = window.children.iter_mut().find(|n| matches!(n, RawNode::Element(e) if local(&e.name) == name)) {
        el.children = vec![RawNode::Text(Arc::from(value))];
        return true;
    }
    if window.attrs.get(name).is_some() {
        window.attrs = RawAttrs::from_pairs(window.attrs.iter().map(|(n, v)| (n, if n == name { value } else { v })));
        return true;
    }
    let rank = ORDER.iter().position(|n| *n == name).unwrap_or(ORDER.len());
    let rank_of = |node: &RawNode| match node {
        RawNode::Element(e) => ORDER.iter().position(|n| *n == local(&e.name)),
        _ => None,
    };
    let at = place_among(window.children.iter().map(rank_of), rank);
    let made = RawElement {
        name: Arc::from(format!("{}{name}", prefix_of(&window.name))),
        attrs: RawAttrs::new(),
        children: vec![RawNode::Text(Arc::from(value))],
    };
    window.children.insert(at.min(window.children.len()), RawNode::Element(made));
    true
}

/// The place a child of the rank `rank` in [`ORDER`] goes among children of the ranks `ranks` (`None` for a child [`ORDER`] does not list): after the last child that comes before
/// it in the order; else before the first that comes after it; else last.
pub(crate) fn place_among<I>(ranks: I, rank: usize) -> usize
where
    I: DoubleEndedIterator<Item = Option<usize>> + ExactSizeIterator + Clone,
{
    let len = ranks.len();
    match ranks.clone().rposition(|r| r.is_some_and(|r| r < rank)) {
        Some(before) => before + 1,
        None => ranks.clone().position(|r| r.is_some_and(|r| r > rank)).unwrap_or(len),
    }
}

/// An `ID` for a window made in `root`: one more than the highest whole-number `ID` of its windows, or 0 when they have none (an `ID` past `u32::MAX` takes the next free one
/// below it, never wrapping).
fn next_id(root: &RawElement) -> String {
    let ids: Vec<u32> = windows(root).filter_map(|w| w.attrs.get("ID")?.trim().parse::<u32>().ok()).collect();
    next_free(ids).to_string()
}

/// The number [`next_id`] gives for windows of the whole-number `ID`s `ids`: one more than the highest, 0 for none, and the least that is not one of them when the highest is
/// `u32::MAX`.
pub(crate) fn next_free(mut ids: Vec<u32>) -> u32 {
    let Some(&most) = ids.iter().max() else { return 0 };
    if let Some(next) = most.checked_add(1) {
        return next;
    }
    // The highest is taken: the least that is not, found in the ids sorted (one pass, however many windows a part holds).
    ids.sort_unstable();
    ids.dedup();
    let mut want = 0u32;
    for id in ids {
        if id != want {
            break;
        }
        match want.checked_add(1) {
            Some(next) => want = next,
            None => break,
        }
    }
    want
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::package::VsdxHeld;

    fn element(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
        RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
    }

    fn text(t: &str) -> RawNode {
        RawNode::Text(Arc::from(t))
    }

    fn flag_element(name: &str, value: &str) -> RawNode {
        RawNode::Element(element(name, &[], vec![text(value)]))
    }

    fn window(attrs: &[(&str, &str)], flags: &[(&str, &str)]) -> RawNode {
        RawNode::Element(element("Window", attrs, flags.iter().map(|(name, value)| flag_element(name, value)).collect()))
    }

    fn names(window: &RawElement) -> Vec<String> {
        window
            .children
            .iter()
            .filter_map(|n| match n {
                RawNode::Element(e) => Some(e.name.to_string()),
                _ => None,
            })
            .collect()
    }

    fn first_window(root: &RawElement) -> &RawElement {
        windows(root).next().unwrap()
    }

    #[test]
    fn the_flags_of_the_drawing_window_are_read() {
        let root = element(
            "Windows",
            &[("ClientWidth", "1")],
            vec![window(&[("ID", "0"), ("WindowType", "Drawing")], &[("ShowRulers", "1"), ("ShowGrid", "0")])],
        );
        assert_eq!(
            WindowFlags::of(&root),
            WindowFlags { rulers: Some(true), grid: Some(false), page_breaks: None, guides: None, connection_points: None }
        );
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowGuides", "0")])]);
        assert_eq!(WindowFlags::of(&root).guides, Some(false));
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing"), ("ShowGuides", "1")], &[])]);
        assert_eq!(WindowFlags::of(&root).guides, Some(true), "as an attribute too");
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowConnectionPoints", "1")])]);
        assert_eq!(WindowFlags::of(&root).connection_points, Some(true));
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowGrid", " true\n")])]);
        assert_eq!(WindowFlags::of(&root).grid, Some(true), "blanks around the value do not matter");
    }

    /// `ShowPageBreaks` is the fifth flag. It is read as a child or an attribute, set where the window holds it, and one the window lacks goes in at its
    /// place in the order, after `ShowGrid` and before `ShowGuides`; nothing else of the part changes.
    #[test]
    fn show_page_breaks_is_read_and_written_as_the_four_are() {
        let drawing = [("ID", "0"), ("WindowType", "Drawing")];
        let child = element("Windows", &[], vec![window(&drawing, &[("ShowGrid", "1"), ("ShowPageBreaks", "1"), ("ShowGuides", "0")])]);
        assert_eq!(WindowFlags::of(&child).page_breaks, Some(true));
        let attribute = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "Drawing"), ("ShowPageBreaks", "0")], &[])]);
        assert_eq!(WindowFlags::of(&attribute).page_breaks, Some(false), "as an attribute too");
        let off = WindowFlags { page_breaks: Some(false), ..WindowFlags::default() };
        // In place: the child's text, the attribute's value.
        let out = off.written(&child).unwrap();
        assert_eq!(names(first_window(&out)), ["ShowGrid", "ShowPageBreaks", "ShowGuides"]);
        assert_eq!(WindowFlags::of(&out), WindowFlags { grid: Some(true), page_breaks: Some(false), guides: Some(false), ..WindowFlags::default() });
        let on = WindowFlags { page_breaks: Some(true), ..WindowFlags::default() };
        let out = on.written(&attribute).unwrap();
        assert_eq!(first_window(&out).attrs.get("ShowPageBreaks"), Some("1"));
        assert!(first_window(&out).children.is_empty(), "no child is added beside the attribute");
        // A value the window holds already writes nothing.
        assert_eq!(on.written(&child), None);
        // A missing one goes between the grid and the guides, whatever else the window holds.
        let lacking = element(
            "Windows",
            &[],
            vec![window(&drawing, &[("StencilGroup", "x"), ("ShowRulers", "1"), ("ShowGrid", "0"), ("ShowGuides", "1"), ("SnapSettings", "65847")])],
        );
        let out = on.written(&lacking).unwrap();
        assert_eq!(names(first_window(&out)), ["StencilGroup", "ShowRulers", "ShowGrid", "ShowPageBreaks", "ShowGuides", "SnapSettings"]);
        let mut back = first_window(&out).clone();
        back.children.retain(|n| !matches!(n, RawNode::Element(e) if &*e.name == "ShowPageBreaks"));
        assert_eq!(&back, first_window(&lacking), "nothing else of the window changed");
        let lacking_all = element("Windows", &[], vec![window(&drawing, &[("ShowRulers", "1"), ("ShowGrid", "1")])]);
        assert_eq!(names(first_window(&on.written(&lacking_all).unwrap())), ["ShowRulers", "ShowGrid", "ShowPageBreaks"]);
        assert!(!on.is_empty() && on.over(&WindowFlags::default()) == on);
        assert_eq!(WindowFlags::default().over(&on).page_breaks, Some(true));
    }

    /// The drawing window is found among others: no other window stands in for it. A part whose windows are a stencil's, or are marked as no type, holds no
    /// settings (read the first window as the drawing's).
    #[test]
    fn the_drawing_window_is_found_among_others_and_no_other_window_stands_in_for_it() {
        let root = element(
            "Windows",
            &[],
            vec![
                window(&[("ID", "1"), ("WindowType", "Stencil")], &[("ShowGrid", "0")]),
                window(&[("ID", "0"), ("WindowType", "drawing")], &[("ShowGrid", "1")]),
            ],
        );
        assert_eq!(WindowFlags::of(&root).grid, Some(true));
        assert_eq!(WindowFlags::held_by(&root).and_then(|f| f.grid), Some(true));
        let stencil = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "Stencil")], &[("ShowGrid", "1"), ("ShowRulers", "0")])]);
        assert_eq!(WindowFlags::held_by(&stencil), None, "a stencil's window is no drawing window");
        assert_eq!(WindowFlags::of(&stencil), WindowFlags::default());
        let unmarked = element("Windows", &[], vec![window(&[("ID", "0")], &[("ShowGrid", "1"), ("ShowRulers", "0")])]);
        assert_eq!(WindowFlags::held_by(&unmarked), None, "nor is a window marked as no type");
    }

    /// As amended: a window whose `WindowType` is the number 1, which Learn's enumeration of window types gives a drawing window
    /// (`visDrawing`), is a drawing window as the word is; the numbers of the other types (2 a stencil, 3 a sheet) and any other text are not. Flags are read from it and written into it,
    /// and no second drawing window is made beside it.
    #[test]
    fn a_window_of_type_one_is_a_drawing_window_as_the_word_is() {
        let one = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "1")], &[("ShowGrid", "1"), ("ShowRulers", "0")])]);
        assert_eq!(WindowFlags::held_by(&one).map(|f| (f.grid, f.rulers)), Some((Some(true), Some(false))), "its flags are the drawing's");
        let bare = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "1")], &[])]);
        assert_eq!(WindowFlags::held_by(&bare), Some(WindowFlags::default()), "a window of type 1 with no flag holds settings, each with no value");
        // It stands first when a stencil's window comes before it, and the word's window after it is not the one read.
        let both = element(
            "Windows",
            &[],
            vec![
                window(&[("ID", "2"), ("WindowType", "Stencil")], &[("ShowGrid", "0")]),
                window(&[("ID", "0"), ("WindowType", "1")], &[("ShowGrid", "1")]),
                window(&[("ID", "1"), ("WindowType", "Drawing")], &[("ShowGrid", "0")]),
            ],
        );
        assert_eq!(WindowFlags::of(&both).grid, Some(true), "the first drawing window, whichever way it is marked");
        for other in ["2", "3", "0", "11", "-1", "1.0", "one", ""] {
            let root = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", other)], &[("ShowGrid", "1")])]);
            assert_eq!(WindowFlags::held_by(&root), None, "WindowType {other:?} is no drawing window");
        }
        // A flag is written into it, and the part gains no window.
        let written = WindowFlags { grid: Some(false), ..WindowFlags::default() }.written(&one).unwrap();
        assert_eq!(windows(&written).count(), 1, "no second drawing window");
        assert_eq!(WindowFlags::of(&written).grid, Some(false));
        assert_eq!(first_window(&written).attrs.get("WindowType"), Some("1"), "the window keeps its type");
        // What it holds already writes nothing.
        assert_eq!(WindowFlags { grid: Some(true), ..WindowFlags::default() }.written(&one), None);
    }

    #[test]
    fn a_prefix_does_not_matter_and_another_root_is_not_the_part() {
        let prefixed = element(
            "v:Windows",
            &[],
            vec![RawNode::Element(element("v:Window", &[("WindowType", "Drawing")], vec![flag_element("v:ShowGrid", "1")]))],
        );
        assert_eq!(WindowFlags::of(&prefixed).grid, Some(true));
        let other = element("Settings", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowGrid", "1")])]);
        assert_eq!(WindowFlags::of(&other), WindowFlags::default());
        assert_eq!(WindowFlags { grid: Some(false), ..WindowFlags::default() }.written(&other), None, "another root is not written into");
    }

    #[test]
    fn an_empty_part_and_odd_values_give_nothing_and_a_document_says_whether_it_holds_a_part() {
        assert_eq!(WindowFlags::of(&element("Windows", &[], Vec::new())), WindowFlags::default());
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowGrid", "maybe")])]);
        assert_eq!(WindowFlags::of(&root), WindowFlags::default());
        let root = element("Windows", &[], vec![text("text, no window"), RawNode::Comment(Arc::from("a comment"))]);
        assert_eq!(WindowFlags::of(&root), WindowFlags::default());
        assert_eq!(WindowFlags::held_by(&root), None, "a part with no window holds no settings");
        assert_eq!(Document::empty().window_flags(), None, "a drawing from no VSDX file");
        let mut held_nothing = Document::empty();
        held_nothing.vsdx = Some(Arc::new(VsdxHeld::default()));
        assert_eq!(held_nothing.window_flags(), None, "a VSDX file whose windows part is not held as a tree");
        let holding = |part: RawElement| {
            let mut doc = Document::empty();
            doc.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::new(part)), ..VsdxHeld::default() }));
            doc
        };
        assert_eq!(holding(element("Windows", &[], Vec::new())).window_flags(), None, "A part with no window holds no settings");
        let stencil = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "Stencil")], &[])]);
        assert_eq!(holding(stencil).window_flags(), None, "nor does one with a stencil's window alone");
        let blank = element("Windows", &[], vec![window(&[("ID", "0"), ("WindowType", "Drawing")], &[])]);
        assert_eq!(
            holding(blank).window_flags(),
            Some(WindowFlags::default()),
            "a drawing window that holds no value holds settings, each with no value"
        );
    }

    /// A flag the window holds is changed where it is, and everything else stays: the other children, the blanks between them, the attributes, the other windows.
    #[test]
    fn a_flag_is_changed_where_it_is_and_nothing_else_changes() {
        let drawing = RawNode::Element(element(
            "Window",
            &[("ID", "0"), ("WindowType", "Drawing"), ("ShowGuides", "1")],
            vec![
                text("\n  "),
                RawNode::Element(element("ShowGrid", &[("x", "y")], vec![text("1")])),
                text("\n  "),
                flag_element("SnapSettings", "295"),
            ],
        ));
        let root = element("Windows", &[("ClientWidth", "800")], vec![window(&[("ID", "1"), ("WindowType", "Stencil")], &[]), drawing]);
        let wanted = WindowFlags { grid: Some(false), guides: Some(false), ..WindowFlags::default() };
        let out = wanted.written(&root).unwrap();
        assert_eq!(WindowFlags::of(&out), WindowFlags { grid: Some(false), guides: Some(false), ..WindowFlags::default() });
        let RawNode::Element(w) = &out.children[1] else { panic!() };
        assert_eq!(w.attrs.get("ShowGuides"), Some("0"), "the attribute is changed where it is");
        assert_eq!(w.attrs.iter().map(|(n, _)| n).collect::<Vec<_>>(), ["ID", "WindowType", "ShowGuides"]);
        let RawNode::Element(grid) = &w.children[1] else { panic!() };
        assert_eq!((grid.attrs.get("x"), &grid.children[..]), (Some("y"), &[text("0")][..]), "the child keeps its attributes");
        assert_eq!(w.children[0], text("\n  "));
        assert_eq!(w.children[2], text("\n  "));
        assert_eq!(out.children[0], root.children[0], "the stencil window is not touched");
        assert_eq!(out.attrs, root.attrs);
    }

    #[test]
    fn a_value_that_is_there_already_changes_nothing() {
        let root = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("ShowGrid", "true"), ("ShowRulers", "0")])]);
        assert_eq!(WindowFlags { grid: Some(true), rulers: Some(false), ..WindowFlags::default() }.written(&root), None);
        assert_eq!(WindowFlags::default().written(&root), None, "setting nothing changes nothing");
        assert!(WindowFlags { grid: Some(false), ..WindowFlags::default() }.written(&root).is_some());
    }

    /// A flag the window lacks goes in as a child at its place in the order, in the window's prefix.
    #[test]
    fn a_missing_flag_goes_in_at_its_place() {
        let root = element(
            "v:Windows",
            &[],
            vec![RawNode::Element(element(
                "v:Window",
                &[("WindowType", "Drawing")],
                vec![
                    flag_element("v:StencilGroup", "1"),
                    flag_element("v:ShowGrid", "1"),
                    flag_element("v:GlueSettings", "9"),
                    flag_element("v:Other", "x"),
                ],
            ))],
        );
        let all = WindowFlags { rulers: Some(true), grid: Some(true), page_breaks: None, guides: Some(false), connection_points: Some(true) };
        let out = all.written(&root).unwrap();
        assert_eq!(
            names(first_window(&out)),
            ["v:StencilGroup", "v:ShowRulers", "v:ShowGrid", "v:ShowGuides", "v:ShowConnectionPoints", "v:GlueSettings", "v:Other"]
        );
        assert_eq!(WindowFlags::of(&out), all);
        // A window with no child of the list: last. One with only later children: before the first of them.
        let bare = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("Unknown", "1")])]);
        assert_eq!(
            names(first_window(&WindowFlags { grid: Some(false), ..WindowFlags::default() }.written(&bare).unwrap())),
            ["Unknown", "ShowGrid"]
        );
        let later = element("Windows", &[], vec![window(&[("WindowType", "Drawing")], &[("SnapSettings", "1"), ("TabSplitterPos", "0.5")])]);
        assert_eq!(
            names(first_window(&WindowFlags { rulers: Some(false), ..WindowFlags::default() }.written(&later).unwrap())),
            ["ShowRulers", "SnapSettings", "TabSplitterPos"]
        );
    }

    /// A part with no drawing window gains one, holding the flags set and no other (the part held no settings, and no other window stands in, so none is copied).
    #[test]
    fn a_part_with_no_drawing_window_gains_one() {
        let empty = element("Windows", &[("ClientWidth", "1")], Vec::new());
        let out = WindowFlags { grid: Some(false), ..WindowFlags::default() }.written(&empty).unwrap();
        let made = first_window(&out);
        assert_eq!((made.attrs.get("ID"), made.attrs.get("WindowType")), (Some("0"), Some("Drawing")));
        assert_eq!(WindowFlags::of(&out), WindowFlags { grid: Some(false), ..WindowFlags::default() });
        let stencils =
            element("Windows", &[], vec![window(&[("ID", "3"), ("WindowType", "Stencil")], &[("ShowRulers", "1")]), window(&[("ID", "7")], &[])]);
        let out = WindowFlags { grid: Some(true), ..WindowFlags::default() }.written(&stencils).unwrap();
        assert_eq!(out.children.len(), 3, "the stencil windows stay");
        assert_eq!(
            WindowFlags::held_by(&out),
            Some(WindowFlags { grid: Some(true), ..WindowFlags::default() }),
            "the stencil's flag is not the drawing's"
        );
        let RawNode::Element(made) = &out.children[2] else { panic!() };
        assert_eq!(made.attrs.get("ID"), Some("8"));
        // A flag a stencil's window holds is no value of the drawing's: setting it makes the window.
        assert!(WindowFlags { rulers: Some(true), ..WindowFlags::default() }.written(&stencils).is_some());
    }

    /// The drawing window of another part (a new drawing's) is put after the last window of a part that holds none, with an `ID` of its own and in
    /// the part's prefix, and nothing else of the part changes; a part that holds one already, another root and a part to take from that holds none give nothing.
    #[test]
    fn the_drawing_window_of_another_part_is_put_into_one_that_holds_none() {
        let new = element(
            "Windows",
            &[("xmlns", "urn:x")],
            vec![RawNode::Element(element(
                "Window",
                &[("ID", "0"), ("WindowType", "Drawing"), ("ContainerType", "Page"), ("Page", "1")],
                vec![flag_element("ShowRulers", "1"), flag_element("ShowGrid", "1")],
            ))],
        );
        let stencil = element(
            "v:Windows",
            &[("ClientWidth", "1680")],
            vec![text("\r\n  "), RawNode::Element(element("v:Window", &[("ID", "0"), ("WindowType", "Stencil")], Vec::new())), text("\r\n")],
        );
        let out = with_drawing_window(&stencil, &new).unwrap();
        assert_eq!(out.attrs, stencil.attrs);
        assert_eq!(
            (&out.children[0], &out.children[1], &out.children[3]),
            (&stencil.children[0], &stencil.children[1], &stencil.children[2]),
            "the rest stays where it was"
        );
        let RawNode::Element(made) = &out.children[2] else { panic!("{out:?}") };
        assert_eq!(&*made.name, "v:Window");
        assert_eq!(
            made.attrs.iter().collect::<Vec<_>>(),
            [("ID", "1"), ("WindowType", "Drawing"), ("ContainerType", "Page"), ("Page", "1")],
            "the next ID"
        );
        assert_eq!(names(made), ["v:ShowRulers", "v:ShowGrid"]);
        assert_eq!(WindowFlags::held_by(&out), Some(WindowFlags { rulers: Some(true), grid: Some(true), ..WindowFlags::default() }));
        // An empty part takes it as its one child.
        let empty = element("Windows", &[], Vec::new());
        let out = with_drawing_window(&empty, &new).unwrap();
        assert_eq!((out.children.len(), first_window(&out).attrs.get("ID")), (1, Some("0")));
        // Nothing to do.
        assert_eq!(with_drawing_window(&new, &new), None, "a part that holds a drawing window already");
        assert_eq!(with_drawing_window(&element("Settings", &[], Vec::new()), &new), None, "another root");
        assert_eq!(with_drawing_window(&empty, &stencil), None, "a part to take from that holds none");
    }

    #[test]
    fn a_new_window_id_never_wraps() {
        let full = element("Windows", &[], vec![window(&[("ID", "4294967295")], &[]), window(&[("ID", "0")], &[])]);
        assert_eq!(next_id(&full), "1");
        let odd = element("Windows", &[], vec![window(&[("ID", "x")], &[])]);
        assert_eq!(next_id(&odd), "0");
    }

    #[test]
    fn the_prefix_of_a_name() {
        assert_eq!(prefix_of("v:Window"), "v:");
        assert_eq!(prefix_of("Window"), "");
        assert_eq!(prefix_of("a:b:Window"), "a:b:");
    }
}
