//! The window settings a drawing holds: whether its window shows the rulers, the grid, the guides and the connection points (the View
//! tab's Ruler, Grid and Guides, and its Visual Aids' Connection Points).
//!
//! **They live in the model**, as the VSDX windows part holds them: the flags of the drawing `Window` in [`diacraft_doc::VsdxHeld::windows`] ([`WindowFlags`]). The command
//! `window.settings.set` writes them there (`cmd/window.rs`), the window shows what [`view_settings`] reads and writes through the command when a box is turned, and so
//! every save writes them: a VSDX file's windows part (spliced in, `vsdx`'s `sync_windows`), a `.diacraft` file's model, an `.odg` file's DiaCraft document file
//! (`odg`'s `ext::docfile`, DiaCraft's own data: no item of `settings.xml` is read for them yet).
//!
//! **What a drawing shows** ([`view_settings`]):
//! - a drawing that holds **no windows part** (a new one, one from an `.odg` or a `.diacraft` file that never had one, a VSDX file without one) shows what a new drawing
//!   shows, [`NEW_DRAWING`]: the grid, the rulers and the guides on, the connection points off; so does one whose part **holds no drawing window** (an
//!   empty `Windows`, or a stencil's window alone), which holds no settings;
//! - one whose part holds a drawing window shows each flag that window holds, and **where it holds none** ([`NO_VALUE`]): the grid and the rulers off (a project rule),
//!   the guides on (ours: off would hide what the person laid on the page), the connection points off (ours).
//!
//! **Turning one changes the drawing and is no undo step** (the owner's ruling, 2026-11-22, "Unsaved, not undo"; and the save specs do not say whether a view change
//! makes a drawing unsaved, and this is the choice that never loses a setting that was turned): the drawing is unsaved, so a save keeps the setting and closing it asks first,
//! and **no undo step is recorded** and **no revision moves** ([`crate::Session::edit_view`]). **Undo and redo leave the settings alone** ([`carry_settings`]): the document
//! they go back to or on to holds the windows part the drawing holds now. A value the drawing shows already writes nothing: the drawing stays as it was, so a drawing whose
//! settings were not turned writes its windows part as it read it.
//!
//! **The document settings of View > Visual Aids follow the same rule**: the **AutoConnect box** (`autoconnect.set`, the `autoconnect`
//! child of DiaCraft's data) and **the Snap & Glue options** (`snap.set`: the switches and the strengths in the `snap` child, and the options in the words of `DocumentSettings`,
//! Dynamic Grid among them) are no undo step and make the drawing unsaved. Undo and redo carry them across too ([`carry_settings`], what the drawing holds being [`HeldSettings`]).
//! What stays an undo step is the shapes' own edits, the guides (a guide is a page shape) and the Reroute settings (a connector's own cells).
//!
//! **The Page Breaks box** (View, Show, Page Breaks; as built it) is a fifth setting of the drawing window, `ShowPageBreaks`
//! ([`page_breaks_held`], [`set_page_breaks`]): it goes there where the drawing has a VSDX place for it, and into DiaCraft's own data where it has none, as it did in
//! every format before. It is no undo step either, and is carried across one as the rest are.

use std::sync::Arc;

use diacraft_doc::{Document, PAGE_BREAKS_CHILD, RawAttrs, RawElement, SnapWords, VsdxHeld, WindowFlags};

/// What the four View settings show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSettings {
    /// The rulers (View, Show, Ruler).
    pub rulers: bool,
    /// The grid (View, Show, Grid).
    pub grid: bool,
    /// The guides (View, Show, Guides).
    pub guides: bool,
    /// The connection points of the shapes in view, whatever the tool (View, Visual Aids, Connection Points). Off, they are marked only while the Connector tool is on or a
    /// connector's end is dragged.
    pub connection_points: bool,
}

/// What a drawing that holds no windows part shows: what a new drawing shows (the new drawing).
pub const NEW_DRAWING: ViewSettings = ViewSettings { rulers: true, grid: true, guides: true, connection_points: false };

/// What a drawing that holds a windows part shows of a flag its drawing window holds no value for.
pub const NO_VALUE: ViewSettings = ViewSettings { rulers: false, grid: false, guides: true, connection_points: false };

/// What `doc` shows: see the module documentation.
pub fn view_settings(doc: &Document) -> ViewSettings {
    shown_by(doc.window_flags())
}

/// What a drawing whose windows part holds the flags `flags` of its drawing window shows, `None` being a drawing that holds no settings (no windows part, or a part with no drawing
/// window): [`NEW_DRAWING`] for none, and each flag that is held, [`NO_VALUE`]'s where it is not.
pub(crate) fn shown_by(flags: Option<WindowFlags>) -> ViewSettings {
    match flags {
        None => NEW_DRAWING,
        Some(flags) => ViewSettings {
            rulers: flags.rulers.unwrap_or(NO_VALUE.rulers),
            grid: flags.grid.unwrap_or(NO_VALUE.grid),
            guides: flags.guides.unwrap_or(NO_VALUE.guides),
            connection_points: flags.connection_points.unwrap_or(NO_VALUE.connection_points),
        },
    }
}

/// The windows part `doc` holds: the settings' place in the model.
pub(crate) fn windows_part(doc: &Document) -> Option<&Arc<RawElement>> {
    doc.vsdx.as_deref().and_then(|held| held.windows.as_ref())
}

/// `target` (a document the history goes back to or on to) with the windows part `windows` (the one the drawing holds now): **undo and redo leave the window settings alone**, because
/// a setting turned is no step of the history and the steps hold the documents as they were. `target` itself when it holds that part already, by pointer or by value (so that an undo
/// back to the saved drawing is the saved drawing, and is clean when the settings are the saved ones); another document, sharing all but the part, when it does not. That one
/// is another allocation than the saved one, so the drawing is unsaved: its settings are not the ones the file holds.
pub(crate) fn carry_windows_part(target: Arc<Document>, windows: Option<Arc<RawElement>>) -> Arc<Document> {
    let same = match (windows_part(&target), windows.as_ref()) {
        (Some(have), Some(want)) => Arc::ptr_eq(have, want) || have == want,
        (None, None) => true,
        _ => false,
    };
    if same {
        return target;
    }
    let mut doc = Document::clone(&target);
    match windows {
        Some(part) => Arc::make_mut(doc.vsdx.get_or_insert_with(Default::default)).windows = Some(part),
        None => {
            if let Some(held) = doc.vsdx.as_mut() {
                Arc::make_mut(held).windows = None;
            }
            // A document that held the part alone holds nothing now: it is the drawing that came from no VSDX file again.
            if doc.vsdx.as_deref().is_some_and(|held| *held == VsdxHeld::default()) {
                doc.vsdx = None;
            }
        }
    }
    Arc::new(doc)
}

/// What a drawing holds of **the settings that are no undo step** (which extends the owner's of 2026-11-22): its windows part, the
/// AutoConnect box and the Snap & Glue settings. Undo and redo give the document they go to what the drawing holds now ([`carry_settings`]), so a step of the history, which is a whole
/// document, never takes a setting back.
#[derive(Clone, Debug)]
pub(crate) struct HeldSettings {
    /// The windows part (the grid, the rulers, the guides and the connection points).
    windows: Option<Arc<RawElement>>,
    /// The attributes of the `autoconnect` child of DiaCraft's data: `on="0"` for a box turned off.
    autoconnect: Option<RawAttrs>,
    /// The attributes of the `page_breaks` child of DiaCraft's data: `on="1"` for a box turned on.
    page_breaks: Option<RawAttrs>,
    /// The attributes of the `snap` child of DiaCraft's data: the Snap and Glue switches and the snap strengths that are not the defaults.
    snap_part: Option<RawAttrs>,
    /// The attributes of the `quick_shapes` child of DiaCraft's data: the Quick Shapes the drawing set (`stencil/quick.rs`).
    quick: Option<RawAttrs>,
    /// The words of `DocumentSettings` that hold the options of Snap & Glue (`SnapSettings`, `GlueSettings`, `SnapExtensions` and `DynamicGridEnabled`).
    words: SnapWords,
}

impl HeldSettings {
    /// What `doc` holds.
    pub(crate) fn of(doc: &Document) -> HeldSettings {
        HeldSettings {
            windows: windows_part(doc).cloned(),
            autoconnect: doc.diacraft_child(AUTOCONNECT_CHILD).map(|child| child.attrs.clone()),
            page_breaks: doc.diacraft_child(PAGE_BREAKS_CHILD).map(|child| child.attrs.clone()),
            snap_part: doc.diacraft_child(crate::snap::settings::PART_CHILD).map(|child| child.attrs.clone()),
            quick: crate::stencil::quick::held_attrs(doc),
            words: doc.snap_words(),
        }
    }
}

/// Makes the child of DiaCraft's data called `name` hold the attributes `attrs` in `doc` (`None` takes it out). It is the child as the settings write it: attributes and nothing in it.
fn put_child(doc: &mut Document, name: &str, attrs: Option<&RawAttrs>) {
    // `false` is data of a version this build does not change, which holds none of these children to begin with: there is nothing to carry into it.
    let _ = match attrs {
        Some(attrs) => {
            let pairs: Vec<(&str, &str)> = attrs.iter().collect();
            doc.set_diacraft_child(name, Some(&pairs))
        }
        None => doc.set_diacraft_child(name, None),
    };
}

/// `target` (a document the history goes back to or on to) holding the settings `held` (what the drawing holds now): **undo and redo leave the settings that are no undo step alone**.
/// The windows part is [`carry_windows_part`]'s. The AutoConnect box, the Snap & Glue switches and strengths, and the words of `DocumentSettings` are compared by what they say (the
/// attributes of the child and the text of the words, not the prefix of a name), and `target` itself is the document when it holds them as `held` does already, so that an undo back to
/// the saved drawing is that drawing and is clean when the settings are the saved ones. Another document, sharing all but them, when it does not: it is another allocation than the
/// saved one, so the drawing is unsaved, its settings not being the ones the file holds.
pub(crate) fn carry_settings(target: Arc<Document>, held: &HeldSettings) -> Arc<Document> {
    let target = carry_windows_part(target, held.windows.clone());
    let child = |doc: &Document, name: &str| doc.diacraft_child(name).map(|child| child.attrs.clone());
    let same = child(&target, AUTOCONNECT_CHILD) == held.autoconnect
        && child(&target, PAGE_BREAKS_CHILD) == held.page_breaks
        && child(&target, crate::snap::settings::PART_CHILD) == held.snap_part
        && child(&target, crate::stencil::quick::CHILD) == held.quick
        && target.snap_words() == held.words;
    if same {
        return target;
    }
    let mut doc = Document::clone(&target);
    put_child(&mut doc, AUTOCONNECT_CHILD, held.autoconnect.as_ref());
    put_child(&mut doc, PAGE_BREAKS_CHILD, held.page_breaks.as_ref());
    put_child(&mut doc, crate::snap::settings::PART_CHILD, held.snap_part.as_ref());
    put_child(&mut doc, crate::stencil::quick::CHILD, held.quick.as_ref());
    doc.set_snap_words(&held.words);
    Arc::new(doc)
}

/// The words a save of a flat OpenDocument drawing (`.fodg`) adds when the drawing's window settings were set: a flat file has no document file of DiaCraft's, which is where an
/// `.odg` keeps them, so the file opens again as a new drawing shows.
pub(crate) const NOT_IN_A_FLAT_FILE: &str = "the window's settings (grid, rulers, guides, connection points, and page breaks the window holds) are not kept in a flat drawing (.fodg): it opens again with the grid, the rulers and the guides on and the connection points off";

/// The words of a drawing whose file holds a windows part DiaCraft does not read (not XML, or another root): it is kept as the bytes it is, and the settings have no place.
pub(crate) const HELD_AS_BYTES: &str =
    "this drawing's file holds a windows part that DiaCraft does not read (it is kept as it is), so the window's settings cannot be kept in it";

/// Whether `doc` came from a VSDX file whose windows part the reader kept as bytes: the package's document has a relationship to a windows part, and the model holds no tree of it.
fn windows_held_as_bytes(doc: &Document) -> bool {
    doc.vsdx.as_deref().is_some_and(|held| held.windows.is_none() && held.rels.iter().any(|r| r.kind.ends_with("/windows")))
}

/// What is `shown`, as the four flags of a window (the Page Breaks box is not one of the four: it is left as the window holds it, [`set_page_breaks`]).
pub(crate) fn every_flag(shown: ViewSettings) -> WindowFlags {
    WindowFlags {
        rulers: Some(shown.rulers),
        grid: Some(shown.grid),
        page_breaks: None,
        guides: Some(shown.guides),
        connection_points: Some(shown.connection_points),
    }
}

/// The flags of `wanted` that differ from what is `shown`: each one `wanted` holds and `shown` does not show, and none for a flag `wanted` leaves out or holds as it is shown.
/// The four View flags only: the Page Breaks box is set by [`set_page_breaks`].
pub(crate) fn change_from(wanted: &WindowFlags, shown: ViewSettings) -> WindowFlags {
    let differs = |want: Option<bool>, now: bool| want.filter(|v| *v != now);
    WindowFlags {
        rulers: differs(wanted.rulers, shown.rulers),
        grid: differs(wanted.grid, shown.grid),
        page_breaks: None,
        guides: differs(wanted.guides, shown.guides),
        connection_points: differs(wanted.connection_points, shown.connection_points),
    }
}

/// Sets in `doc` each flag `wanted` holds that differs from what `doc` shows (see the module documentation): → whether `doc` changed. A drawing with no windows part is given
/// a new drawing's ([`diacraft_vsdx::new_windows_part`], which shows [`NEW_DRAWING`]) before the flags are set in it, and **a part that holds no drawing window** (which holds no
/// settings, and shows [`NEW_DRAWING`] too) is given that part's drawing window, after its own windows ([`diacraft_doc::with_drawing_window`]), so that what the drawing
/// shows changes only where a flag was set. `Err` (in words) for a drawing whose file's windows part is held as bytes, which has no place for them.
pub(crate) fn set(doc: &mut Document, wanted: &WindowFlags) -> Result<bool, &'static str> {
    let shown = view_settings(doc);
    let change = change_from(wanted, shown);
    if change.is_empty() {
        return Ok(false);
    }
    if windows_held_as_bytes(doc) {
        return Err(HELD_AS_BYTES);
    }
    put_in_window(doc, change, shown)
}

/// Writes the flags `change` holds into the drawing window of `doc`'s windows part, which [`set`] and [`set_page_breaks`] share: a drawing with no part is given a new drawing's
/// first, and a part with no drawing window that part's drawing window; a drawing with no page, which has no such window to take, gets a window holding every flag as it is
/// `shown`, with `change` over them. → `Ok(true)`; `Err` (in words) for a part that is no `Windows` the flags can be written into.
fn put_in_window(doc: &mut Document, mut change: WindowFlags, shown: ViewSettings) -> Result<bool, &'static str> {
    let mut current = match doc.vsdx.as_deref().and_then(|held| held.windows.clone()) {
        Some(windows) => windows,
        None => Arc::new(diacraft_vsdx::new_windows_part(doc)),
    };
    if WindowFlags::held_by(&current).is_none() {
        if let Some(made) = diacraft_doc::with_drawing_window(&current, &diacraft_vsdx::new_windows_part(doc)) {
            current = Arc::new(made);
        }
        // A drawing with no page has no new drawing's window to take: the window made holds every flag as it is shown, the ones set over them.
        if WindowFlags::held_by(&current).is_none() {
            change = change.over(&every_flag(shown));
        }
    }
    let made = change.written(&current).ok_or(HELD_AS_BYTES)?;
    let held = Arc::make_mut(doc.vsdx.get_or_insert_with(Default::default));
    held.windows = Some(Arc::new(made));
    Ok(true)
}

/// The child of the extension data that holds the AutoConnect box, and its attribute: `on` is `0` or `1`.
const AUTOCONNECT_CHILD: &str = "autoconnect";

/// The words of a drawing whose extension data is of a version this build does not change: the AutoConnect box has no place in it.
pub(crate) const AUTOCONNECT_NOT_KEPT: &str =
    "this drawing holds extension data of a version this build does not change (it is kept as it is), so the AutoConnect box cannot be kept in it";

/// What `doc` holds for the AutoConnect box (View, Visual Aids, AutoConnect): `Some(false)` when it was turned off, `Some(true)` when the data says on, `None` when it
/// holds nothing.
pub fn autoconnect_held(doc: &Document) -> Option<bool> {
    let child = doc.diacraft_child(AUTOCONNECT_CHILD)?;
    match child.attrs.get("on")?.trim() {
        "0" | "false" => Some(false),
        "1" | "true" => Some(true),
        _ => None,
    }
}

/// Whether AutoConnect is on for `doc` (ours): **on for a drawing that holds nothing**. The specification reads the box as the file's own
/// switch under the program-wide "Enable AutoConnect", which "is on by default"; no specification says where a file keeps the box or what a
/// file that does not say has, so a drawing that says nothing has it on, and only an off is written.
pub fn autoconnect_on(doc: &Document) -> bool {
    autoconnect_held(doc).unwrap_or(true)
}

/// Sets the AutoConnect box of `doc`: off is written into the drawing's extension data (a namespaced extension, which every format carries: no specification gives the
/// box a place in a VSDX or OpenDocument file), on takes the data out again, so a drawing whose box is on holds nothing of it. -> whether `doc` changed; `Err` (in
/// words) for a drawing whose extension data is of a version this build does not change.
pub(crate) fn set_autoconnect(doc: &mut Document, on: bool) -> Result<bool, &'static str> {
    if autoconnect_on(doc) == on {
        return Ok(false);
    }
    let kept = if on { doc.set_diacraft_child(AUTOCONNECT_CHILD, None) } else { doc.set_diacraft_child(AUTOCONNECT_CHILD, Some(&[("on", "0")])) };
    if kept { Ok(true) } else { Err(AUTOCONNECT_NOT_KEPT) }
}

/// The words of a drawing whose extension data is of a version this build does not change: the Page Breaks box has no place in it.
pub(crate) const PAGE_BREAKS_NOT_KEPT: &str =
    "this drawing holds extension data of a version this build does not change (it is kept as it is), so the Page Breaks box cannot be kept in it";

/// What `doc` holds for the Page Breaks box (View, Show, Page Breaks): **the drawing window's `ShowPageBreaks`** where it holds one (a VSDX file's own value, or
/// one DiaCraft wrote there), else what DiaCraft's own data says ([`diacraft_doc::PAGE_BREAKS_CHILD`], where kept the box in every format, and where a drawing
/// with no VSDX place for it keeps it still), else `None`.
pub fn page_breaks_held(doc: &Document) -> Option<bool> {
    doc.window_flags().and_then(|flags| flags.page_breaks).or_else(|| doc.page_breaks_in_data())
}

/// Whether the Page Breaks box of `doc` goes into its drawing window: **the drawing came from a VSDX file** (its model holds the package's own rows, not a
/// windows part alone), whose window is the box's place in that format, or **its drawing window holds a value for the box already** (a part a VSDX
/// file gave a drawing saved since as another format, or one a new drawing's VSDX save wrote), which is read first and so is the one to change. Not for a drawing whose
/// file's windows part is held as bytes, which has no place for it. Every other drawing (a new one, an `.odg`, a flat `.fodg`, a `.diacraft` of no VSDX file) keeps the
/// box in DiaCraft's own data, as built it: a flat drawing keeps that data and not a windows part.
fn page_breaks_in_window(doc: &Document) -> bool {
    let Some(held) = doc.vsdx.as_deref() else { return false };
    !windows_held_as_bytes(doc) && (!held.holds_windows_alone() || doc.window_flags().is_some_and(|flags| flags.page_breaks.is_some()))
}

/// Whether `doc` shows its page breaks (ours): **off for a drawing that holds nothing**. The specification states no default for the desktop application
/// (the published images vary) and the page breaks of a page that is one sheet are none, so a drawing that says nothing has them off, and only an on is written.
pub fn page_breaks_on(doc: &Document) -> bool {
    page_breaks_held(doc).unwrap_or(false)
}

/// Sets the Page Breaks box of `doc`. **Where the drawing has a VSDX place for it** ([`page_breaks_in_window`]) it is the drawing window's `ShowPageBreaks`, set
/// where the window holds it or put in at its place in the order (a part or a drawing window made as [`set`] makes them), and DiaCraft's own child of it is taken out, so that
/// the box has one place and a VSDX file's DiaCraft data holds nothing the format can say; a file whose settings were not turned keeps its windows part byte for byte. **Elsewhere**
/// it is written into DiaCraft's own extension data, a namespaced one that every format carries, as the AutoConnect box is: on is written, off takes the data out again, so a
/// drawing whose box is off holds nothing of it. -> whether `doc` changed; `Err` (in words) for a drawing whose extension data is of a version this build does not change and that
/// has no window to hold the box.
pub(crate) fn set_page_breaks(doc: &mut Document, on: bool) -> Result<bool, &'static str> {
    if page_breaks_on(doc) == on {
        return Ok(false);
    }
    if page_breaks_in_window(doc) {
        let change = WindowFlags { page_breaks: Some(on), ..WindowFlags::default() };
        // A part that is no `Windows` takes nothing (and nothing of `doc` changed): the box goes into DiaCraft's data below.
        let shown = view_settings(doc);
        if put_in_window(doc, change, shown).is_ok() {
            // `false`: data of a version this build does not change, kept as it is; the window's value is the one read.
            let _ = doc.set_diacraft_child(PAGE_BREAKS_CHILD, None);
            return Ok(true);
        }
    }
    let kept = if on { doc.set_diacraft_child(PAGE_BREAKS_CHILD, Some(&[("on", "1")])) } else { doc.set_diacraft_child(PAGE_BREAKS_CHILD, None) };
    if kept { Ok(true) } else { Err(PAGE_BREAKS_NOT_KEPT) }
}

#[cfg(test)]
mod tests {
    use diacraft_doc::{RawAttrs, RawElement, RawNode, Rel, VsdxHeld};

    use super::*;

    fn windows(flags: &[(&str, &str)]) -> Arc<RawElement> {
        let children = flags
            .iter()
            .map(|(name, value)| {
                RawNode::Element(RawElement { name: Arc::from(*name), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(*value))] })
            })
            .collect();
        let window = RawElement { name: Arc::from("Window"), attrs: RawAttrs::from_pairs([("ID", "0"), ("WindowType", "Drawing")]), children };
        Arc::new(RawElement { name: Arc::from("Windows"), attrs: RawAttrs::new(), children: vec![RawNode::Element(window)] })
    }

    fn holding(part: Arc<RawElement>) -> Document {
        let mut doc = Document::new_blank();
        doc.vsdx = Some(Arc::new(VsdxHeld { windows: Some(part), ..VsdxHeld::default() }));
        doc
    }

    #[test]
    fn no_part_shows_a_new_drawings_settings_and_a_part_shows_its_own() {
        assert_eq!(view_settings(&Document::new_blank()), NEW_DRAWING);
        assert_eq!(view_settings(&holding(windows(&[]))), NO_VALUE, "a part whose window holds no value");
        let all = holding(windows(&[("ShowRulers", "0"), ("ShowGrid", "1"), ("ShowGuides", "0"), ("ShowConnectionPoints", "1")]));
        assert_eq!(view_settings(&all), ViewSettings { rulers: false, grid: true, guides: false, connection_points: true });
    }

    #[test]
    fn a_new_drawings_part_shows_what_no_part_shows() {
        let doc = Document::new_blank();
        let made = holding(Arc::new(diacraft_vsdx::new_windows_part(&doc)));
        assert_eq!(view_settings(&made), NEW_DRAWING);
    }

    #[test]
    fn a_value_shown_already_changes_nothing_and_another_is_written() {
        let mut doc = Document::new_blank();
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(true), rulers: Some(true), ..WindowFlags::default() }), Ok(false));
        assert!(doc.vsdx.is_none(), "no part is made for what a new drawing shows");
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(false), ..WindowFlags::default() }), Ok(true));
        assert_eq!(view_settings(&doc), ViewSettings { grid: false, ..NEW_DRAWING });
        assert!(doc.vsdx.as_deref().unwrap().holds_windows_alone(), "the part alone: nothing of a package");
        // A part whose window holds no value shows the grid off: off writes nothing, on writes the flag.
        let mut doc = holding(windows(&[]));
        let before = doc.clone();
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(false), ..WindowFlags::default() }), Ok(false));
        assert_eq!(doc, before);
        assert_eq!(set(&mut doc, &WindowFlags { guides: Some(false), connection_points: Some(true), ..WindowFlags::default() }), Ok(true));
        assert_eq!(view_settings(&doc), ViewSettings { rulers: false, grid: false, guides: false, connection_points: true });
    }

    /// A part that holds no drawing window (a stencil's window alone, or none) shows a new drawing's settings, and a flag set in it puts a new drawing's drawing window in
    /// after the stencil's, so that only the flag set changes what the drawing shows. A drawing with no page, which has no such window to take, gets a window with every flag.
    #[test]
    fn a_part_with_no_drawing_window_shows_a_new_drawings_and_is_given_its_window_when_a_flag_is_set() {
        let stencil = RawElement {
            name: Arc::from("Windows"),
            attrs: RawAttrs::new(),
            children: vec![RawNode::Element(RawElement {
                name: Arc::from("Window"),
                attrs: RawAttrs::from_pairs([("ID", "0"), ("WindowType", "Stencil")]),
                children: vec![RawNode::Element(RawElement {
                    name: Arc::from("ShowRulers"),
                    attrs: RawAttrs::new(),
                    children: vec![RawNode::Text(Arc::from("0"))],
                })],
            })],
        };
        let mut doc = holding(Arc::new(stencil.clone()));
        assert_eq!(view_settings(&doc), NEW_DRAWING, "a stencil's flag is not the drawing's");
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(true), rulers: Some(true), ..WindowFlags::default() }), Ok(false), "shown already");
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(false), ..WindowFlags::default() }), Ok(true));
        assert_eq!(view_settings(&doc), ViewSettings { grid: false, ..NEW_DRAWING });
        let part = windows_part(&doc).unwrap();
        let windows: Vec<&RawElement> = part
            .children
            .iter()
            .filter_map(|n| match n {
                RawNode::Element(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(windows.len(), 2, "{part:?}");
        assert_eq!(windows[0].attrs.get("WindowType"), Some("Stencil"));
        let made = windows[1];
        assert_eq!((made.attrs.get("ID"), made.attrs.get("WindowType"), made.attrs.get("ContainerType")), (Some("1"), Some("Drawing"), Some("Page")));
        assert_eq!(made.attrs.get("Page"), doc.pages.iter().next().map(|p| p.id.to_string()).as_deref(), "the first page's, as a new drawing's");
        // A drawing with no page: every flag as it is shown, the one set over them.
        let mut bare = Document::empty();
        bare.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::new(RawElement { children: Vec::new(), ..stencil })), ..VsdxHeld::default() }));
        assert!(bare.pages.is_empty());
        assert_eq!(set(&mut bare, &WindowFlags { rulers: Some(false), ..WindowFlags::default() }), Ok(true));
        assert_eq!(view_settings(&bare), ViewSettings { rulers: false, ..NEW_DRAWING });
        assert_eq!(
            bare.window_flags(),
            Some(WindowFlags { rulers: Some(false), grid: Some(true), page_breaks: None, guides: Some(true), connection_points: Some(false) })
        );
    }

    #[test]
    fn a_part_held_as_bytes_is_refused_in_words() {
        let mut doc = Document::new_blank();
        let rel = Rel {
            source: Arc::from("/visio/document.xml"),
            id: Arc::from("rId2"),
            kind: Arc::from("http://schemas.microsoft.com/visio/2010/relationships/windows"),
            target: Arc::from("windows.xml"),
            external: false,
        };
        doc.vsdx = Some(Arc::new(VsdxHeld { rels: vec![rel], ..VsdxHeld::default() }));
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(false), ..WindowFlags::default() }), Err(HELD_AS_BYTES));
        assert!(HELD_AS_BYTES.contains("windows part") && HELD_AS_BYTES.contains("cannot be kept"));
        // What the drawing shows already is no change, and no refusal.
        assert_eq!(set(&mut doc, &WindowFlags { grid: Some(true), ..WindowFlags::default() }), Ok(false));
    }
    #[test]
    fn a_target_that_holds_the_part_already_is_the_very_target() {
        let part = windows(&[("ShowGrid", "0")]);
        let target = Arc::new(holding(Arc::clone(&part)));
        assert!(Arc::ptr_eq(&carry_windows_part(Arc::clone(&target), Some(part)), &target), "the same part");
        let equal = windows(&[("ShowGrid", "0")]);
        assert!(
            Arc::ptr_eq(&carry_windows_part(Arc::clone(&target), Some(equal)), &target),
            "an equal part is the part: an undo back to the saved drawing is that drawing"
        );
        let none = Arc::new(Document::new_blank());
        assert!(Arc::ptr_eq(&carry_windows_part(Arc::clone(&none), None), &none), "no part on either side");
    }

    #[test]
    fn a_target_with_another_part_is_given_the_part_and_nothing_else_changes() {
        let (old, new) = (windows(&[("ShowGrid", "1")]), windows(&[("ShowGrid", "0")]));
        let target = Arc::new(holding(old));
        let carried = carry_windows_part(Arc::clone(&target), Some(Arc::clone(&new)));
        assert!(!Arc::ptr_eq(&carried, &target));
        assert!(Arc::ptr_eq(windows_part(&carried).unwrap(), &new));
        assert!(!view_settings(&carried).grid);
        let mut back = Document::clone(&carried);
        back.vsdx = target.vsdx.clone();
        assert_eq!(&back, &*target, "only the windows part differs");
        // A target that came from no VSDX file holds the part alone.
        let blank = carry_windows_part(Arc::new(Document::new_blank()), Some(new));
        assert!(blank.vsdx.as_deref().unwrap().holds_windows_alone());
        // The rows of a package are kept when the part is put in.
        let mut held = VsdxHeld::default();
        held.rels.push(Rel {
            source: Arc::from("/visio/document.xml"),
            id: Arc::from("rId2"),
            kind: Arc::from("http://example.test/relationship"),
            target: Arc::from("x.xml"),
            external: false,
        });
        let mut package = Document::new_blank();
        package.vsdx = Some(Arc::new(held));
        let carried = carry_windows_part(Arc::new(package), Some(windows(&[])));
        assert_eq!(carried.vsdx.as_deref().unwrap().rels.len(), 1);
        assert!(!carried.vsdx.as_deref().unwrap().holds_windows_alone());
    }

    /// A drawing that holds the AutoConnect box turned off, a Snap switch turned off, a strength and the words of `DocumentSettings`.
    fn holding_settings() -> Document {
        let mut doc = Document::new_blank();
        assert!(doc.set_diacraft_child("autoconnect", Some(&[("on", "0")])));
        assert!(doc.set_diacraft_child("snap", Some(&[("snap", "0"), ("glue", "0"), ("strength_grid", "20")])));
        doc.set_snap_words(&SnapWords {
            glue: Some(Arc::from("9")),
            snap: Some(Arc::from("65845")),
            extensions: Some(Arc::from("34")),
            dynamic_grid: Some(Arc::from("0")),
        });
        doc
    }

    #[test]
    fn a_target_that_holds_the_settings_already_is_the_very_target() {
        let target = Arc::new(holding_settings());
        let held = HeldSettings::of(&target);
        assert!(Arc::ptr_eq(&carry_settings(Arc::clone(&target), &held), &target), "the same settings");
        // A document of its own that says the same (another prefix on its children, another allocation) is the same too: an undo back to the saved drawing is that drawing.
        let mut other = holding_settings();
        let mut data = other.diacraft.as_deref().cloned().unwrap();
        for child in &mut data.children {
            if let RawNode::Element(el) = child {
                let local = el.name.rsplit_once(':').map_or(&*el.name, |(_, local)| local).to_owned();
                el.name = Arc::from(format!("y:{local}"));
            }
        }
        other.diacraft = Some(Arc::new(data));
        let other = Arc::new(other);
        assert!(Arc::ptr_eq(&carry_settings(Arc::clone(&other), &held), &other), "settings are compared by what they say");
        let none = Arc::new(Document::new_blank());
        assert!(Arc::ptr_eq(&carry_settings(Arc::clone(&none), &HeldSettings::of(&none)), &none), "no settings on either side");
    }

    #[test]
    fn a_target_with_other_settings_is_given_them_and_nothing_else_changes() {
        let held = HeldSettings::of(&holding_settings());
        let plain = Arc::new(Document::new_blank());
        let carried = carry_settings(Arc::clone(&plain), &held);
        assert!(!Arc::ptr_eq(&carried, &plain));
        assert_eq!(carried.diacraft_child("autoconnect").and_then(|c| c.attrs.get("on")), Some("0"));
        assert_eq!(carried.diacraft_child("snap").and_then(|c| c.attrs.get("strength_grid")), Some("20"));
        assert_eq!(carried.snap_words(), holding_settings().snap_words());
        // The settings go the other way too: a target that holds them is given none, and holds no data of DiaCraft's and no words when nothing else is held.
        let target = Arc::new(holding_settings());
        let carried = carry_settings(Arc::clone(&target), &HeldSettings::of(&plain));
        assert!(carried.diacraft.is_none(), "every child is taken out, and the data with them");
        assert_eq!(carried.snap_words(), SnapWords::default());
        let mut back = Document::clone(&carried);
        back.diacraft = target.diacraft.clone();
        back.passthrough = target.passthrough.clone();
        assert_eq!(&back, &*target, "only the settings differ");
        // A child of the data that is not a setting stays where it is.
        let mut other = holding_settings();
        assert!(other.set_diacraft_child("elsewhere", Some(&[("k", "v")])));
        let carried = carry_settings(Arc::new(other), &HeldSettings::of(&plain));
        assert!(
            carried.diacraft_child("elsewhere").is_some()
                && carried.diacraft_child("snap").is_none()
                && carried.diacraft_child("autoconnect").is_none()
        );
    }

    #[test]
    fn the_windows_part_is_carried_with_the_rest() {
        let (old, new) = (windows(&[("ShowGrid", "1")]), windows(&[("ShowGrid", "0")]));
        let mut now = holding_settings();
        now.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::clone(&new)), ..VsdxHeld::default() }));
        let target = Arc::new(holding(old));
        let carried = carry_settings(target, &HeldSettings::of(&now));
        assert!(Arc::ptr_eq(windows_part(&carried).unwrap(), &new), "the part");
        assert_eq!(carried.diacraft_child("autoconnect").and_then(|c| c.attrs.get("on")), Some("0"), "and the box");
    }

    #[test]
    fn a_drawing_with_no_part_takes_the_part_away_and_holds_nothing_when_nothing_else_is_held() {
        let alone = Arc::new(holding(windows(&[("ShowGrid", "0")])));
        let carried = carry_windows_part(alone, None);
        assert!(carried.vsdx.is_none(), "a part alone, taken away, leaves the drawing that came from no VSDX file");
        assert_eq!(view_settings(&carried), NEW_DRAWING);
        let mut held = VsdxHeld { windows: Some(windows(&[])), ..VsdxHeld::default() };
        held.props_rest.push((Arc::from("Company"), diacraft_doc::Kept::default()));
        let mut package = Document::new_blank();
        package.vsdx = Some(Arc::new(held));
        let carried = carry_windows_part(Arc::new(package), None);
        assert!(
            carried.vsdx.as_deref().is_some_and(|held| held.windows.is_none() && held.props_rest.len() == 1),
            "what else the package holds stays"
        );
    }

    // ---- the Page Breaks box ----

    /// A drawing read from a VSDX package: it holds a row of the package's own (so not a windows part alone), and the windows part `part` if any.
    fn from_a_package(part: Option<Arc<RawElement>>) -> Document {
        let rel = Rel {
            source: Arc::from("/visio/document.xml"),
            id: Arc::from("rId1"),
            kind: Arc::from("http://example.test/relationship"),
            target: Arc::from("x.xml"),
            external: false,
        };
        let mut doc = Document::new_blank();
        doc.vsdx = Some(Arc::new(VsdxHeld { windows: part, rels: vec![rel], ..VsdxHeld::default() }));
        doc
    }

    fn breaks_in_window(doc: &Document) -> Option<bool> {
        doc.window_flags().and_then(|f| f.page_breaks)
    }

    /// A drawing from a VSDX file keeps the box in its drawing window's `ShowPageBreaks` (read, set in place, put in at its place), never in DiaCraft's data; a drawing with no
    /// VSDX place keeps it in DiaCraft's data as before; the window's value is read before the data's.
    #[test]
    fn the_page_breaks_box_is_the_vsdx_windows_where_there_is_one_and_diacrafts_data_elsewhere() {
        // A VSDX file whose window says the page breaks show.
        let mut doc = from_a_package(Some(windows(&[("ShowGrid", "1"), ("ShowPageBreaks", "1")])));
        assert!(page_breaks_on(&doc) && page_breaks_held(&doc) == Some(true), "the file's own value is read");
        assert_eq!(set_page_breaks(&mut doc, true), Ok(false), "shown already: nothing is written");
        assert_eq!(set_page_breaks(&mut doc, false), Ok(true));
        assert_eq!(breaks_in_window(&doc), Some(false), "set where the window holds it");
        assert!(doc.diacraft.is_none(), "and nothing in DiaCraft's data");
        // A VSDX file whose window holds no value: the flag goes in, and the grid is left as it was.
        let mut doc = from_a_package(Some(windows(&[("ShowGrid", "0")])));
        assert!(!page_breaks_on(&doc));
        assert_eq!(set_page_breaks(&mut doc, true), Ok(true));
        assert_eq!(breaks_in_window(&doc), Some(true));
        assert_eq!(view_settings(&doc), ViewSettings { grid: false, ..NO_VALUE });
        assert!(doc.diacraft.is_none());
        // A VSDX file with no windows part is given one, as a turned grid gives one.
        let mut doc = from_a_package(None);
        assert_eq!(set_page_breaks(&mut doc, true), Ok(true));
        assert_eq!(breaks_in_window(&doc), Some(true));
        assert_eq!(view_settings(&doc), NEW_DRAWING, "the other settings show what they showed");
        // A file saved by the build that kept the box in DiaCraft's data: read from there, and moved into the window at the first turn.
        let mut doc = from_a_package(Some(windows(&[("ShowGrid", "1")])));
        assert!(doc.set_diacraft_child(PAGE_BREAKS_CHILD, Some(&[("on", "1")])));
        assert!(page_breaks_on(&doc), "the data is read where the window holds no value");
        assert_eq!(set_page_breaks(&mut doc, false), Ok(true));
        assert_eq!((breaks_in_window(&doc), doc.page_breaks_in_data()), (Some(false), None), "one place: the window");
        // The window's value is read before the data's.
        assert!(doc.set_diacraft_child(PAGE_BREAKS_CHILD, Some(&[("on", "1")])));
        assert!(!page_breaks_on(&doc), "the window says off");
        // A new drawing, and one whose grid was turned (its windows part alone): DiaCraft's data, as before.
        let mut doc = Document::new_blank();
        assert_eq!(set_page_breaks(&mut doc, true), Ok(true));
        assert_eq!((doc.vsdx.is_none(), doc.page_breaks_in_data()), (true, Some(true)), "no windows part is made for it");
        assert!(set(&mut doc, &WindowFlags { grid: Some(false), ..WindowFlags::default() }).unwrap());
        assert!(page_breaks_on(&doc), "a part made for the grid keeps the box on");
        assert_eq!(set_page_breaks(&mut doc, false), Ok(true));
        assert!(!page_breaks_on(&doc));
        // A part alone whose window holds the box (a VSDX drawing saved as `.odg` and read again): the window is the one to change.
        let mut doc = holding(windows(&[("ShowPageBreaks", "1")]));
        assert_eq!(set_page_breaks(&mut doc, false), Ok(true));
        assert_eq!((breaks_in_window(&doc), doc.page_breaks_in_data()), (Some(false), None));
        // A file whose windows part is held as bytes has no window for it: DiaCraft's data.
        let mut doc = from_a_package(None);
        let held = Arc::make_mut(doc.vsdx.as_mut().unwrap());
        held.rels[0].kind = Arc::from("http://schemas.microsoft.com/visio/2010/relationships/windows");
        assert_eq!(set_page_breaks(&mut doc, true), Ok(true));
        assert_eq!((doc.vsdx.as_deref().unwrap().windows.is_none(), doc.page_breaks_in_data()), (true, Some(true)));
    }

    /// Undo and redo carry the box in the window as they carry the other settings: a document the history goes to is given the part the drawing holds now.
    #[test]
    fn the_box_in_the_window_is_carried_across_an_undo() {
        let mut now = from_a_package(Some(windows(&[])));
        let target = Arc::new(now.clone());
        assert_eq!(set_page_breaks(&mut now, true), Ok(true));
        let carried = carry_settings(Arc::clone(&target), &HeldSettings::of(&now));
        assert!(page_breaks_on(&carried) && !page_breaks_on(&target));
    }
}
