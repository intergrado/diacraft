//! What a document, page, master or shape says about itself: the properties of `docProps`, the page
//! functions, `NAME`, `MASTERNAME`, `ID`, `TYPE` and the rest.
//!
//! Each reads the sheet it is evaluated on, which is the inheritor: the same call in a master's shape and in its instance
//! answers for each. Two answers differ from MS-VSDX on purpose, as the samples decide: on a master's sheet
//! `PAGENUMBER` is the master's place in the master list and `PAGENAME` the master's name. `FILENAME` and `DIRECTORY` are the
//! machine's, so they come from the options ([`crate::FileInfo`]).

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{CellAddr, Owner, RowKey, ShapeType, SheetRef};
use diacraft_units::Unit;

use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::rules::BAD_ARGUMENT_ERROR;
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

/// Text of a few bytes made here (a constant, a number in a name): copying it is not the cost of a step.
fn text(s: &str) -> Value {
    Value::text(s)
}

/// Text the file sets, shared and not copied: a name, a property, a shape's data. Handing it on costs nothing, and whatever reads or cuts it
/// charges its length (`TITLE()` of 16 MiB copied the title at one step a call).
fn shared(s: &Arc<str>) -> Value {
    Value::Text(Arc::clone(s))
}

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

/// What a sheet is, for the functions that ask which page or master it is on.
enum Where {
    Page(u32),
    Master(u32),
    Shape(diacraft_doc::ShapeRef),
    Document,
    Style(u32),
}

fn locate(sheet: &SheetRef) -> Where {
    match sheet {
        SheetRef::Document => Where::Document,
        SheetRef::Style(id) => Where::Style(*id),
        SheetRef::Page(id) => Where::Page(*id),
        SheetRef::Master(id) => Where::Master(*id),
        SheetRef::Shape(s) => Where::Shape(s.clone()),
    }
}

/// The part a sheet is in: a page or a master, if it is in one.
fn owner(w: &Where) -> Option<Owner> {
    match w {
        Where::Page(id) => Some(Owner::Page(*id)),
        Where::Master(id) => Some(Owner::Master(*id)),
        Where::Shape(s) => Some(s.owner),
        Where::Document | Where::Style(_) => None,
    }
}

/// Whether the one optional argument asks for the universal form: exactly 750, once cut to a whole number; anything else, and no
/// argument, is the local form.
fn universal(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Result<bool, Value> {
    match slots.first() {
        Some(Some(v)) if v.is_failure() => Err(v.clone()),
        // A word, a Boolean and anything else that is no number asks for the local form.
        Some(Some(v)) => Ok(ev.read_double_lenient(v, site.env).is_ok_and(|q| q.stored.trunc() == 750.0)),
        _ => Ok(false),
    }
}

/// The name asked for, falling back to the other when the one asked for is missing. Shared, not copied.
fn pick(name: Option<&Arc<str>>, name_u: Option<&Arc<str>>, universal: bool) -> Arc<str> {
    let (first, second) = if universal { (name_u, name) } else { (name, name_u) };
    first.or(second).map_or_else(|| Arc::from(""), Arc::clone)
}

/// A `.NNN` suffix cut from a name: `Pump.17` is `Pump`.
fn cut_suffix(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((head, tail)) if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => name,
    }
}

fn pagename(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let uni = match universal(ev, site, slots) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match owner(&locate(&site.target.sheet)) {
        Some(Owner::Page(id)) => ev.res.page(id).map_or_else(|| text(""), |p| Value::Text(pick(Some(&p.name), p.name_u.as_ref(), uni))),
        // A master's sheet answers with the master's name.
        Some(Owner::Master(id)) => ev.res.master(id).map_or_else(|| text(""), |m| Value::Text(pick(m.name.as_ref(), m.name_u.as_ref(), uni))),
        None => text(""),
    }
}

fn bkgpagename(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let uni = match universal(ev, site, slots) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let none = || text("<no background>");
    let Some(Owner::Page(id)) = owner(&locate(&site.target.sheet)) else { return none() };
    let Some(back) = ev.res.page(id).and_then(|p| p.back_page) else { return none() };
    ev.res.page(back).map_or_else(none, |p| Value::Text(pick(Some(&p.name), p.name_u.as_ref(), uni)))
}

fn pagenumber(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    match owner(&locate(&site.target.sheet)) {
        Some(Owner::Page(id)) => {
            let mut place = 0u32;
            // The scan of the pages is charged by their number.
            if !ev.work(ev.doc.pages.len()) {
                return Value::Error(crate::rules::LOOP_ERROR);
            }
            for p in ev.doc.pages.iter() {
                if !p.background {
                    place += 1;
                }
                if p.id == id {
                    // A background page has no number.
                    return Value::num(if p.background { 0.0 } else { f64::from(place) });
                }
            }
            Value::num(0.0)
        }
        // A master's sheet answers with the master's place in the master index.
        Some(Owner::Master(id)) => {
            if !ev.work(ev.doc.masters.len()) {
                return Value::Error(crate::rules::LOOP_ERROR);
            }
            Value::num(ev.doc.masters.iter().position(|m| m.id == id).map_or(0.0, |i| (i + 1) as f64))
        }
        None => Value::num(0.0),
    }
}

fn pagecount(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    if !ev.work(ev.doc.pages.len()) {
        return Value::Error(crate::rules::LOOP_ERROR);
    }
    Value::num(ev.doc.pages.iter().filter(|p| !p.background).count() as f64)
}

/// The master of the nearest shape, the one asked or above it, that holds `Master`.
fn master_of(ev: &Evaluator<'_, '_>, sheet: &SheetRef) -> Option<u32> {
    let SheetRef::Shape(at) = sheet else { return None };
    if !matches!(at.owner, Owner::Page(_)) {
        return None;
    }
    // The nearest shape, the one asked first and then each group above it, that holds `Master`: through the resolver's maps of parts.
    let mut cur = at.clone();
    loop {
        if let Some(master) = ev.res.shape(&cur)?.master {
            return Some(master);
        }
        cur = cur.parent()?;
    }
}

fn mastername(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let uni = match universal(ev, site, slots) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let none = || text("<no master>");
    match master_of(ev, &site.target.sheet).and_then(|id| ev.res.master(id)) {
        Some(m) => Value::Text(pick(m.name.as_ref(), m.name_u.as_ref(), uni)),
        None => none(),
    }
}

/// `NAME([lang])`: the shape's name; a nameless instance answers with its master's name, any `.NNN` cut, a point and its
/// own `ID`; any other nameless shape is `Sheet.<id>`.
fn name(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let uni = match universal(ev, site, slots) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match locate(&site.target.sheet) {
        Where::Shape(at) => {
            let Some(shape) = ev.res.shape(&at) else { return Value::Error(CellError::Ref) };
            if shape.name.is_some() || shape.name_u.is_some() {
                return Value::Text(pick(shape.name.as_ref(), shape.name_u.as_ref(), uni));
            }
            if let Some(master) = shape.master.and_then(|id| ev.res.master(id)) {
                let base = pick(master.name.as_ref(), master.name_u.as_ref(), uni);
                // The master's name is copied to build the instance's: charged by its length.
                if !ev.work(base.len()) {
                    return Value::Error(crate::rules::LOOP_ERROR);
                }
                return text(&format!("{}.{}", cut_suffix(&base), shape.id));
            }
            text(&format!("Sheet.{}", shape.id))
        }
        Where::Style(id) => ev.res.style(id).map_or_else(|| text(""), |s| Value::Text(pick(s.name.as_ref(), s.name_u.as_ref(), uni))),
        Where::Page(id) => ev.res.page(id).map_or_else(|| text(""), |p| Value::Text(pick(Some(&p.name), p.name_u.as_ref(), uni))),
        Where::Master(id) => ev.res.master(id).map_or_else(|| text(""), |m| Value::Text(pick(m.name.as_ref(), m.name_u.as_ref(), uni))),
        Where::Document => text(""),
    }
}

fn id(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    match locate(&site.target.sheet) {
        Where::Shape(at) => ev.res.shape(&at).map_or(Value::Error(CellError::Ref), |s| Value::num(f64::from(s.id))),
        Where::Page(id) | Where::Master(id) | Where::Style(id) => Value::num(f64::from(id)),
        Where::Document => Value::num(0.0),
    }
}

fn is1d(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    if !matches!(locate(&site.target.sheet), Where::Shape(_)) {
        // A page's or the document's sheet has no ends.
        return Value::Error(CellError::Ref);
    }
    let sheet = site.target.sheet.clone();
    let any = ["BeginX", "BeginY", "EndX", "EndY"]
        .iter()
        .any(|cell| ev.res.resolve(&sheet, &CellAddr::direct(cell)).is_some_and(|r| r.formula.cell.is_some() || r.value.cell.is_some()));
    Value::Bool(any)
}

fn kind(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> (f64, &'static str) {
    match locate(&site.target.sheet) {
        Where::Document => (79.0, "Document"),
        Where::Style(_) => (74.0, "Style"),
        Where::Page(_) | Where::Master(_) => (70.0, "Page"),
        Where::Shape(at) => match ev.res.shape(&at).map(|s| &s.kind) {
            Some(ShapeType::Group) => (71.0, "Group"),
            Some(ShapeType::Foreign) => (78.0, "Foreign"),
            Some(ShapeType::Guide) => (77.0, "Guide"),
            _ => (72.0, "Shape"),
        },
    }
}

fn type_code(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    Value::num(kind(ev, site).0)
}

/// `TYPEDESC()`: the object names of `TYPE`'s table (project rule).
fn typedesc(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    text(kind(ev, site).1)
}

/// `VERSION()`: the whole-number part of `AppVersion`; 0 when it is missing or no number.
fn version(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    let n = ev.doc.props.app_version.as_deref().and_then(crate::strnum::parse_decimal).map_or(0.0, f64::trunc);
    Value::num(n)
}

fn prop(ev: &Evaluator<'_, '_>, f: fn(&diacraft_doc::DocProps) -> Option<&Arc<str>>) -> Value {
    f(&ev.doc.props).map_or_else(|| text(""), shared)
}

fn title(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    shared(&ev.doc.title)
}

fn category(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.category.as_ref())
}

fn company(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.company.as_ref())
}

fn creator(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.creator.as_ref())
}

fn description(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.description.as_ref())
}

fn hyperlinkbase(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.hyperlink_base.as_ref())
}

fn keywords(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.keywords.as_ref())
}

fn manager(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.manager.as_ref())
}

fn subject(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    prop(ev, |p| p.subject.as_ref())
}

/// `FILENAME()` and `DIRECTORY()` are the machine's, not the file's: an input. A cell that reads them is the oracle's to skip.
fn filename(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    ev.ctx.clock_touched = true;
    text(ev.ctx.opts.file.name.as_deref().unwrap_or(""))
}

fn directory(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    ev.ctx.clock_touched = true;
    text(ev.ctx.opts.file.directory.as_deref().unwrap_or(""))
}

/// `DATA1`, `DATA2` and `DATA3`: the shape's text of that name, the master shape's when the instance has none, else empty.
fn data(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, which: usize) -> Value {
    let SheetRef::Shape(at) = &site.target.sheet else { return text("") };
    let pick_of = |s: &diacraft_doc::Shape| -> Option<Arc<str>> {
        match which {
            1 => s.data1.clone(),
            2 => s.data2.clone(),
            _ => s.data3.clone(),
        }
    };
    if let Some(own) = ev.res.shape(at).and_then(|s| pick_of(s)) {
        return Value::Text(own);
    }
    if let Ok(Some(step)) = ev.res.master_step(at)
        && let Some(master_shape) = ev.res.shape(&step.shape)
        && let Some(held) = pick_of(master_shape)
    {
        return Value::Text(held);
    }
    text("")
}

fn data1(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    data(ev, site, 1)
}

fn data2(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    data(ev, site, 2)
}

fn data3(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: Slots<'_>) -> Value {
    data(ev, site, 3)
}

/// `HASCATEGORY(list)`: whether the shape's `User.msvShapeCategories` and the argument, each split at `;`, share a category, compared
/// exactly. An empty category never matches.
fn hascategory(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let wanted = match ev.arg_text(slots, 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let sheet = site.target.sheet.clone();
    let read = ev.read_cell(&sheet, &CellAddr::in_row("User", None, RowKey::name("msvShapeCategories"), "Value"));
    let held = match &read.view {
        Value::Text(t) if read.found => Arc::clone(t),
        _ => return Value::Bool(false),
    };
    // Both lists are read once, at 64 bytes a step, and each item is hashed or looked up, a step for each: the lists are the file's, and a
    // search of one for each item of the other cost 24 seconds for 100,000 items each side.
    if !ev.work(held.len() + wanted.len()) {
        return Value::Error(crate::rules::LOOP_ERROR);
    }
    let items = held.bytes().chain(wanted.bytes()).filter(|b| *b == b';').count() + 2;
    if !ev.steps(items) {
        return Value::Error(crate::rules::LOOP_ERROR);
    }
    let have: HashSet<&str> = held.split(';').filter(|c| !c.is_empty()).collect();
    Value::Bool(wanted.split(';').filter(|c| !c.is_empty()).any(|c| have.contains(c)))
}

/// `LANGUAGE(tag or number)`: the LCID. A number passes through; only the page's two tags are known, and any
/// other tag is `#VALUE!`.
fn language(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let Ok(v) = crate::convert::slot(slots, 0) else { return bad() };
    match v {
        // Compared in place: a tag of a megabyte is not copied to be lower-cased, and differs in length from both.
        Value::Text(t) if t.eq_ignore_ascii_case("en-us") => Value::num(1033.0),
        Value::Text(t) if t.eq_ignore_ascii_case("es-es") => Value::num(3082.0),
        Value::Text(_) => Value::Error(CellError::Value),
        other => match ev.read_double(other, site.env) {
            Ok(q) => Value::Number(diacraft_units::Quantity::new(q.stored, Unit::Unitless)),
            Err(e) => e,
        },
    }
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("BKGPAGENAME", 0, 1, bkgpagename),
    eager("CATEGORY", 0, 0, category),
    eager("COMPANY", 0, 0, company),
    eager("CREATOR", 0, 0, creator),
    eager("DATA1", 0, 0, data1),
    eager("DATA2", 0, 0, data2),
    eager("DATA3", 0, 0, data3),
    eager("DESCRIPTION", 0, 0, description),
    eager("DIRECTORY", 0, 0, directory),
    eager("FILENAME", 0, 0, filename),
    eager("HASCATEGORY", 1, 1, hascategory),
    eager("HYPERLINKBASE", 0, 0, hyperlinkbase),
    eager("ID", 0, 0, id),
    eager("IS1D", 0, 0, is1d),
    eager("KEYWORD", 0, 0, keywords),
    eager("KEYWORDS", 0, 0, keywords),
    eager("LANGUAGE", 1, 1, language),
    eager("MANAGER", 0, 0, manager),
    eager("MASTERNAME", 0, 1, mastername),
    eager("NAME", 0, 1, name),
    eager("PAGECOUNT", 0, 0, pagecount),
    eager("PAGENAME", 0, 1, pagename),
    eager("PAGENUMBER", 0, 0, pagenumber),
    eager("SUBJECT", 0, 0, subject),
    eager("TITLE", 0, 0, title),
    eager("TYPE", 0, 0, type_code),
    eager("TYPEDESC", 0, 0, typedesc),
    eager("VERSION", 0, 0, version),
];
