//! The first place two documents differ, as a path with the two values cut short: for the failure message of a property whose models are too large to read.

use std::fmt::Debug;
use std::sync::Arc;

use diacraft_doc::{Cell, Cells, Document, Master, Page, Row, Section, Shape, ShapeList, SharedVec, Sheet, Style};

fn cut(x: &impl Debug) -> String {
    let s = format!("{x:?}");
    if s.len() > 240 { format!("{}...", s.chars().take(240).collect::<String>()) } else { s }
}

fn leaf<T: PartialEq + Debug>(path: &str, a: &T, b: &T) -> Option<String> {
    (a != b).then(|| {
        let (x, y) = (format!("{a:?}"), format!("{b:?}"));
        if x.len() <= 240 && y.len() <= 240 {
            return format!("{path}: {x} != {y}");
        }
        // Long values: the place they first differ, with the text around it.
        let at = x.bytes().zip(y.bytes()).position(|(p, q)| p != q).unwrap_or(x.len().min(y.len()));
        let window = |t: &str| {
            let (lo, hi) = (floor(t, at.saturating_sub(160)), floor(t, (at + 1200).min(t.len())));
            t[lo..hi].to_owned()
        };
        format!("{path}: differ at byte {at}: ...{} != ...{}", window(&x), window(&y))
    })
}

/// The largest char boundary of `t` not past `at`.
fn floor(t: &str, mut at: usize) -> usize {
    while at > 0 && !t.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn cell(path: &str, a: &Cell, b: &Cell) -> Option<String> {
    leaf(&format!("{path}.formula"), &a.formula, &b.formula)
        .or_else(|| leaf(&format!("{path}.value"), &a.value, &b.value))
        .or_else(|| leaf(&format!("{path}.unit"), &a.unit, &b.unit))
        .or_else(|| leaf(&format!("{path}.error"), &a.error, &b.error))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.extra"), &a.extra, &b.extra))
}

fn cells(path: &str, a: &Cells, b: &Cells) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("{path}: {} cells != {}", a.len(), b.len()));
    }
    for ((na, ca), (nb, cb)) in a.iter().zip(b) {
        if na != nb {
            return Some(format!("{path}: name {} != {}", cut(na), cut(nb)));
        }
        if let Some(d) = cell(&format!("{path}[{}]", cut(na)), ca, cb) {
            return Some(d);
        }
    }
    None
}

fn row(path: &str, a: &Row, b: &Row) -> Option<String> {
    leaf(&format!("{path}.name"), &a.name, &b.name)
        .or_else(|| leaf(&format!("{path}.ix"), &a.ix, &b.ix))
        .or_else(|| leaf(&format!("{path}.local_name"), &a.local_name, &b.local_name))
        .or_else(|| leaf(&format!("{path}.row_type"), &a.row_type, &b.row_type))
        .or_else(|| leaf(&format!("{path}.del"), &a.del, &b.del))
        .or_else(|| cells(&format!("{path}.cells"), &a.cells, &b.cells))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn section(path: &str, a: &Section, b: &Section) -> Option<String> {
    leaf(&format!("{path}.name"), &a.name, &b.name)
        .or_else(|| leaf(&format!("{path}.ix"), &a.ix, &b.ix))
        .or_else(|| leaf(&format!("{path}.del"), &a.del, &b.del))
        .or_else(|| cells(&format!("{path}.cells"), &a.cells, &b.cells))
        .or_else(|| {
            if a.rows.len() != b.rows.len() {
                return Some(format!("{path}.rows: {} != {}", a.rows.len(), b.rows.len()));
            }
            a.rows.iter().zip(b.rows.iter()).enumerate().find_map(|(i, (x, y))| row(&format!("{path}.rows[{i}]"), x, y))
        })
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn sheet(path: &str, a: &Sheet, b: &Sheet) -> Option<String> {
    leaf(&format!("{path}.links"), &a.links, &b.links)
        .or_else(|| cells(&format!("{path}.cells"), &a.cells, &b.cells))
        .or_else(|| {
            if a.sections.len() != b.sections.len() {
                return Some(format!("{path}.sections: {} != {}", a.sections.len(), b.sections.len()));
            }
            a.sections.iter().zip(&b.sections).enumerate().find_map(|(i, (x, y))| section(&format!("{path}.sections[{i}]"), x, y))
        })
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn shape(path: &str, a: &Shape, b: &Shape) -> Option<String> {
    leaf(&format!("{path}.id"), &a.id, &b.id)
        .or_else(|| leaf(&format!("{path}.name"), &a.name, &b.name))
        .or_else(|| leaf(&format!("{path}.name_u"), &a.name_u, &b.name_u))
        .or_else(|| leaf(&format!("{path}.kind"), &a.kind, &b.kind))
        .or_else(|| leaf(&format!("{path}.master"), &a.master, &b.master))
        .or_else(|| leaf(&format!("{path}.master_shape"), &a.master_shape, &b.master_shape))
        .or_else(|| leaf(&format!("{path}.del"), &a.del, &b.del))
        .or_else(|| sheet(&format!("{path}.sheet"), &a.sheet, &b.sheet))
        .or_else(|| leaf(&format!("{path}.text"), &a.text, &b.text))
        .or_else(|| leaf(&format!("{path}.data1"), &a.data1, &b.data1))
        .or_else(|| leaf(&format!("{path}.data2"), &a.data2, &b.data2))
        .or_else(|| leaf(&format!("{path}.data3"), &a.data3, &b.data3))
        .or_else(|| leaf(&format!("{path}.foreign"), &a.foreign, &b.foreign))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
        .or_else(|| shapes(&format!("{path}.children"), &a.children, &b.children))
}

fn shapes(path: &str, a: &ShapeList, b: &ShapeList) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("{path}: {} shapes != {}", a.len(), b.len()));
    }
    a.iter().zip(b.iter()).enumerate().find_map(|(i, (x, y))| shape(&format!("{path}[{i}]"), x, y))
}

fn page(path: &str, a: &Page, b: &Page) -> Option<String> {
    leaf(&format!("{path}.id"), &a.id, &b.id)
        .or_else(|| leaf(&format!("{path}.name"), &a.name, &b.name))
        .or_else(|| leaf(&format!("{path}.name_u"), &a.name_u, &b.name_u))
        .or_else(|| leaf(&format!("{path}.background"), &a.background, &b.background))
        .or_else(|| leaf(&format!("{path}.back_page"), &a.back_page, &b.back_page))
        .or_else(|| leaf(&format!("{path}.rel_id"), &a.rel_id, &b.rel_id))
        .or_else(|| leaf(&format!("{path}.part_name"), &a.part_name, &b.part_name))
        .or_else(|| sheet(&format!("{path}.sheet"), &a.sheet, &b.sheet))
        .or_else(|| shapes(&format!("{path}.shapes"), &a.shapes, &b.shapes))
        .or_else(|| leaf(&format!("{path}.connects"), &a.connects, &b.connects))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn master(path: &str, a: &Master, b: &Master) -> Option<String> {
    leaf(&format!("{path}.id"), &a.id, &b.id)
        .or_else(|| leaf(&format!("{path}.name"), &a.name, &b.name))
        .or_else(|| leaf(&format!("{path}.name_u"), &a.name_u, &b.name_u))
        .or_else(|| leaf(&format!("{path}.unique_id"), &a.unique_id, &b.unique_id))
        .or_else(|| leaf(&format!("{path}.pattern_flags"), &a.pattern_flags, &b.pattern_flags))
        .or_else(|| leaf(&format!("{path}.rel_id"), &a.rel_id, &b.rel_id))
        .or_else(|| leaf(&format!("{path}.icon"), &a.icon, &b.icon))
        .or_else(|| sheet(&format!("{path}.sheet"), &a.sheet, &b.sheet))
        .or_else(|| shapes(&format!("{path}.shapes"), &a.shapes, &b.shapes))
        .or_else(|| leaf(&format!("{path}.connects"), &a.connects, &b.connects))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn style(path: &str, a: &Style, b: &Style) -> Option<String> {
    leaf(&format!("{path}.id"), &a.id, &b.id)
        .or_else(|| leaf(&format!("{path}.name"), &a.name, &b.name))
        .or_else(|| leaf(&format!("{path}.name_u"), &a.name_u, &b.name_u))
        .or_else(|| sheet(&format!("{path}.sheet"), &a.sheet, &b.sheet))
        .or_else(|| leaf(&format!("{path}.raw"), &a.raw, &b.raw))
        .or_else(|| leaf(&format!("{path}.passthrough"), &a.passthrough, &b.passthrough))
        .or_else(|| leaf(&format!("{path}.ext"), &a.ext, &b.ext))
}

fn list<T>(path: &str, a: &SharedVec<Arc<T>>, b: &SharedVec<Arc<T>>, each: fn(&str, &T, &T) -> Option<String>) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("{path}: {} != {}", a.len(), b.len()));
    }
    a.iter().zip(b.iter()).enumerate().find_map(|(i, (x, y))| each(&format!("{path}[{i}]"), x, y))
}

/// The first place `a` and `b` differ, or `None` when they are equal.
pub fn first_difference(a: &Document, b: &Document) -> Option<String> {
    leaf("version", &a.version, &b.version)
        .or_else(|| leaf("title", &a.title, &b.title))
        .or_else(|| leaf("props", &a.props, &b.props))
        .or_else(|| list("pages", &a.pages, &b.pages, page))
        .or_else(|| list("masters", &a.masters, &b.masters, master))
        .or_else(|| list("styles", &a.styles, &b.styles, style))
        .or_else(|| leaf("themes", &a.themes, &b.themes))
        .or_else(|| leaf("colors", &a.colors, &b.colors))
        .or_else(|| leaf("fonts", &a.fonts, &b.fonts))
        .or_else(|| sheet("sheet", &a.sheet, &b.sheet))
        .or_else(|| leaf("passthrough", &a.passthrough, &b.passthrough))
        .or_else(|| leaf("diacraft", &a.diacraft, &b.diacraft))
        .or_else(|| leaf("ext", &a.ext, &b.ext))
        .or_else(|| leaf("parts", &a.parts, &b.parts))
        .or_else(|| leaf("vsdx", &a.vsdx, &b.vsdx))
        .or_else(|| (a != b).then(|| "the documents differ in a place this walk does not look at".to_owned()))
}
