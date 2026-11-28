//! The Insert tab's side of the engine: the hyperlinks of a shape (`hyperlink.set`, `hyperlink.list`, `hyperlink.follow`), fields and characters in a shape's text
//! (`text.insert_field`, `text.field_choices`, `text.field_sample`, `text.update_fields`, `text.insert`), and a picture from a file (`insert.picture`). The rules are named in
//! `draw/links.rs`, `draw/fields.rs` and `draw/pictures.rs`, and in `docs/editing-commands.md`.

use serde_json::{Map, Value, json};

use super::params::{optional_number, optional_whole, page_for};
use super::{CommandSpec, has_document, optional_text, required_text, required_u32};
use crate::Session;
use crate::draw::fields::{self, Env, Scope};
use crate::draw::links::{self, Link, LinkSpec, Target};
use crate::draw::pictures::{self, Where};
use crate::draw::{self};
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.picture",
            "Pictures",
            ["Insert", "Illustrations"],
            None,
            "path: text, the picture file: a PNG, JPEG, GIF, BMP, TIFF, EMF or WMF, told by its bytes and not by its name, of at most 64 MiB and 64 megapixels (a GIF or a TIFF is kept and drawn as a \
             placeholder, an SVG is refused); \
             x: number, optional, inches from the page's left edge to the picture's middle (default: the middle of the page, with y); \
             y: number, optional, inches from the page's bottom edge to the picture's middle; \
             width: number, optional, more than 0, the picture's width in inches (default: its own at 96 pixels to the inch, made to fit within 80 percent of the page, and in proportion when only \
             one of width and height is given); height: number, optional, more than 0, the picture's height in inches; \
             page: whole number, optional, the id of the page (default: the first page that is not a background). \
             The picture is a Foreign shape holding the file's bytes as they are (the same bytes held once however many shapes show them), on the active layers, in front, and is one undo step. \
             The pictures of a drawing hold at most 512 MiB between them",
            has_document,
            insert_picture
        ),
        cmd!(
            "hyperlink.set",
            "Hyperlinks",
            ["Insert", "Links"],
            None,
            "id: whole number, the shape; \
             links: array of objects, the shape's whole list of hyperlinks, each a row of its Hyperlink section (an empty list takes them all away). A link that gives name keeps that row \
             (the cells it gives change and the others stay), a link with no name is a new row after the others (named Row_1, Row_2 and so on), and a row the list leaves out is taken away. \
             A link gives description (text, the name its right-click menu lists), address (text, a web http or https, FTP or mailto address or a file, empty for a link in this drawing), \
             sub_address (text, in this drawing a page's name or a shape as <page name>/<shape name> or <page name>/Sheet.<id>, and on a web address the fragment), extra_info (text), \
             frame (text), new_window (true or false), default (true or false, at most one link is the default and a click follows it) and invisible (true or false, the link is left out of \
             the right-click menu). A link needs an address or a sub-address. At most 256 links are taken, and 4096 characters of each text; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            set_links
        ),
        cmd!(
            query "hyperlink.list",
            "List Hyperlinks",
            ["Insert", "Links"],
            None,
            "id: whole number, the shape; page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            list_links
        ),
        cmd!(
            query "hyperlink.follow",
            "Follow Hyperlink",
            ["Insert", "Links"],
            None,
            "id: whole number, the shape; link: text, optional, the name of one of its links (default: the link marked default, else the first that is not invisible); \
             page: whole number, optional, the id of the page (default: the first page with the shape). Says where the link goes and does not go there, the window shows the page or shape \
             and opens the web address. Only web, FTP and mail addresses and the links of this drawing are followed, and a file or another program is refused in words",
            has_document,
            follow_link
        ),
        cmd!(
            "text.insert_field",
            "Field",
            ["Insert", "Text"],
            None,
            "id: whole number, the shape; \
             category: text, one of custom, date_time, document, geometry, object, page, shape_data and user_cell (the dialog's Custom Formula, Date/Time, Document Info, Geometry, Object Info, \
             Page Info, Shape Data and User-defined Cells, and text.field_choices lists the fields of each); \
             field: text, the field of the category (creation, current, last_edit or print of date_time, page_number of page, width of geometry and so on, and for shape_data and user_cell the name \
             of one of the shape's rows), and not given for custom; \
             formula: text, only for custom, the ShapeSheet formula whose result the field shows; \
             format: text, optional, the picture that turns the value into text (default: a text @, a number esc(0), a date and time {<211>}, a length esc(7), an angle esc(12), a shape data row's \
             own when it holds a picture, never none since an empty picture shows nothing, and a blank one is none given, and FIELDPICTURE(n) is the text a format code stands for); \
             at: whole number, optional, the character of the shape's text the field goes before, from 0, not counting its fields (default: the end); \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             The field is a fld of the text and a row of the shape's Field section, shows what its formula gives through its format, and is one undo step",
            has_document,
            insert_field
        ),
        cmd!(
            query "text.field_choices",
            "Field Choices",
            ["Insert", "Text"],
            None,
            "id: whole number, optional, a shape: the Shape Data and User-defined Cells categories list its rows; \
             page: whole number, optional, the id of the page (default: the first page with the shape, else the first page that is not a background)",
            has_document,
            field_choices
        ),
        cmd!(
            query "text.field_sample",
            "Field Sample",
            ["Insert", "Text"],
            None,
            "id: whole number, the shape; category: text, as for text.insert_field; field: text, optional; formula: text, optional; format: text, optional; \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             Says what the field would show, as shown, and changes nothing",
            has_document,
            field_sample
        ),
        cmd!(
            "text.update_fields",
            "Update Fields",
            ["Insert", "Text"],
            None,
            "id: whole number, optional, a shape: only its fields are worked out again; \
             page: whole number, optional, the id of a page: only the fields of its shapes (default: every page's). \
             The text of each field is worked out again from its row, with the clock and the file's name, and what changed is put in as one undo step. \
             A field is also updated by itself when its shape is moved or sized and when a page is added, renamed, taken away, copied or moved",
            has_document,
            update_fields
        ),
        cmd!(
            "text.insert",
            "Insert Text",
            ["Insert", "Text"],
            None,
            "id: whole number, the shape; text: text, the characters to put in (a symbol, say); \
             at: whole number, optional, the character they go before, from 0, not counting the shape's fields (default: the end); \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             The runs and fields of the text are kept, as shape.set_text keeps them for a range, and nothing to put is no undo step",
            has_document,
            insert_text
        ),
    ]
}

/// The page the command works on (see [`page_for`]).
fn target_page(s: &Session, p: &Value, id: u32, cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, &[id], cmd)
}

/// A link as the answers give it.
fn link_json(doc: &diacraft_doc::Document, link: &Link) -> Value {
    json!({
        "name": link.name,
        "description": link.description,
        "address": link.address,
        "sub_address": link.sub_address,
        "extra_info": link.extra_info,
        "frame": link.frame,
        "new_window": link.new_window,
        "default": link.default,
        "invisible": link.invisible,
        "kind": links::kind_of(doc, link),
    })
}

fn text_key(map: &Map<String, Value>, key: &str, cmd: &str, n: usize) -> Result<Option<String>> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(bad(cmd, format!("link {n}: `{key}` must be text"))),
    }
}

fn flag_key(map: &Map<String, Value>, key: &str, cmd: &str, n: usize) -> Result<Option<bool>> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(bad(cmd, format!("link {n}: `{key}` must be true or false"))),
    }
}

/// The links a `links` param gives, each key checked.
fn specs_of(p: &Value, cmd: &str) -> Result<Vec<LinkSpec>> {
    let Some(Value::Array(items)) = p.get("links") else {
        return Err(bad(cmd, "`links` must be an array of links"));
    };
    if items.len() > links::MAX_LINKS {
        return Err(bad(cmd, format!("a shape holds at most {} hyperlinks", links::MAX_LINKS)));
    }
    let mut out = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let n = at + 1;
        let Value::Object(map) = item else {
            return Err(bad(cmd, format!("link {n} must be an object")));
        };
        const KEYS: [&str; 9] = ["name", "description", "address", "sub_address", "extra_info", "frame", "new_window", "default", "invisible"];
        if let Some(unknown) = map.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(bad(cmd, format!("link {n} has an unknown key `{}`; a link takes {}", crate::error::shown(unknown), KEYS.join(", "))));
        }
        out.push(LinkSpec {
            name: text_key(map, "name", cmd, n)?,
            description: text_key(map, "description", cmd, n)?,
            address: text_key(map, "address", cmd, n)?,
            sub_address: text_key(map, "sub_address", cmd, n)?,
            extra_info: text_key(map, "extra_info", cmd, n)?,
            frame: text_key(map, "frame", cmd, n)?,
            new_window: flag_key(map, "new_window", cmd, n)?,
            default: flag_key(map, "default", cmd, n)?,
            invisible: flag_key(map, "invisible", cmd, n)?,
        });
    }
    Ok(out)
}

/// The list the shape has now → `{page, id, links}`.
fn answer(s: &Session, page: u32, id: u32, cmd: &str) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let listed = links::read_links(doc, page, id).map_err(|p| p.into_engine(cmd))?;
    Ok(json!({ "page": page, "id": id, "links": listed.iter().map(|l| link_json(doc, l)).collect::<Vec<_>>() }))
}

/// Sets the shape's hyperlinks → `{page, id, links}`: the list the shape has now. A set that changes nothing is no undo step.
fn set_links(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "hyperlink.set";
    let id = required_u32(p, "id", ID)?;
    let specs = specs_of(p, ID)?;
    let page = target_page(s, p, id, ID)?;
    s.edit_if("Hyperlinks", |doc| {
        let outcome = links::set_links(doc, page, id, &specs).map_err(|problem| problem.into_engine(ID))?;
        Ok(((), outcome.changed))
    })?;
    answer(s, page, id, ID)
}

/// The shape's hyperlinks → `{page, id, links}`.
fn list_links(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "hyperlink.list";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, id, ID)?;
    answer(s, page, id, ID)
}

/// Where a click on the shape goes → `{page, id, link, kind, new_window, ...}`: `kind` is `page` (with `target_page`), `shape` (with `target_page` and `target_shape`), `web` or `mail`
/// (with `url`). Nothing is followed here.
fn follow_link(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "hyperlink.follow";
    let id = required_u32(p, "id", ID)?;
    let name = super::optional_text(p, "link", ID)?;
    let page = target_page(s, p, id, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let listed = links::read_links(doc, page, id).map_err(|p| p.into_engine(ID))?;
    let link = links::pick(&listed, name).map_err(|p| p.into_engine(ID))?;
    let target = links::resolve(doc, link).map_err(|p| p.into_engine(ID))?;
    let mut out = json!({ "page": page, "id": id, "link": link.name, "new_window": link.new_window });
    if let Some(map) = out.as_object_mut() {
        match target {
            Target::Page { page: to } => {
                map.insert("kind".to_owned(), json!("page"));
                map.insert("target_page".to_owned(), json!(to));
            }
            Target::Shape { page: to, shape } => {
                map.insert("kind".to_owned(), json!("shape"));
                map.insert("target_page".to_owned(), json!(to));
                map.insert("target_shape".to_owned(), json!(shape));
            }
            Target::Web { url } => {
                map.insert("kind".to_owned(), json!("web"));
                map.insert("url".to_owned(), json!(url));
            }
            Target::Mail { url } => {
                map.insert("kind".to_owned(), json!("mail"));
                map.insert("url".to_owned(), json!(url));
            }
        }
    }
    Ok(out)
}

/// The clock and the file of the session, for the fields.
fn env_of(s: &Session) -> Env {
    let file = s.source().and_then(|f| {
        let path = f.path();
        Some((path.file_name()?.to_str()?.to_owned(), path.parent()?.to_str()?.to_owned()))
    });
    Env::new(s.now(), file)
}

/// The character place `at` as the number of characters, or `None` for the end.
fn place_of(p: &Value, cmd: &str) -> Result<Option<usize>> {
    optional_whole(p, "at", cmd)?.map(|n| usize::try_from(n).map_err(|_| bad(cmd, "`at` is too large"))).transpose()
}

/// What a field-making command is asked: the plan, checked against the shape.
fn plan_of(s: &Session, p: &Value, page: u32, id: u32, cmd: &str) -> Result<fields::Planned> {
    let category = required_text(p, "category", cmd)?;
    let (field, formula, format) = (optional_text(p, "field", cmd)?, optional_text(p, "formula", cmd)?, optional_text(p, "format", cmd)?);
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let at = draw::require_shape_ref(doc, page, id).map_err(|e| e.into_engine(cmd))?;
    fields::plan(doc, &at, category, field, formula, format).map_err(|e| e.into_engine(cmd))
}

fn inserted_json(page: u32, id: u32, plan: &fields::Planned, made: &fields::Inserted) -> Value {
    json!({
        "page": page,
        "id": id,
        "ix": made.ix,
        "at": made.at,
        "category": plan.category,
        "field": plan.field,
        "formula": made.formula,
        "format": made.format,
        "type": made.kind.word(),
        "shown": made.shown,
    })
}

/// Puts a field in the text → `{page, id, ix, at, category, field, formula, format, type, shown}`.
fn insert_field(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.insert_field";
    let id = required_u32(p, "id", ID)?;
    let at = place_of(p, ID)?;
    let page = target_page(s, p, id, ID)?;
    let plan = plan_of(s, p, page, id, ID)?;
    let env = env_of(s);
    let made = s.edit("Insert field", |doc| fields::insert_field(doc, page, id, at, &plan, &env).map_err(|e| e.into_engine(ID)))?;
    Ok(inserted_json(page, id, &plan, &made))
}

/// What a field would show → `{page, id, shown, formula, format, type}`; nothing changes.
fn field_sample(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.field_sample";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, id, ID)?;
    let plan = plan_of(s, p, page, id, ID)?;
    let env = env_of(s);
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let made = fields::sample(doc, page, id, &plan, &env).map_err(|e| e.into_engine(ID))?;
    Ok(inserted_json(page, id, &plan, &made))
}

/// The categories of the Field dialog with the fields of each, and the formats the Data Format list offers.
fn field_choices(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.field_choices";
    let id = optional_whole(p, "id", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match (id, optional_whole(p, "page", ID)?) {
        (_, Some(page)) => page,
        (Some(id), None) => page_for(doc, None, &[id], ID)?,
        (None, None) => doc.first_foreground_page().map(|pg| pg.id).ok_or_else(|| bad(ID, "the document has no pages"))?,
    };
    let mut categories = Vec::with_capacity(fields::CATEGORIES.len());
    for category in &fields::CATEGORIES {
        let listed = fields::choices_of(doc, page, id, category).map_err(|e| e.into_engine(ID))?;
        let entries: Vec<Value> = listed
            .into_iter()
            .map(|(key, label, formula, kind, format)| json!({"id": key, "label": label, "formula": formula, "type": kind.word(), "format": format}))
            .collect();
        categories.push(json!({"id": category.id, "label": category.label, "fields": entries}));
    }
    let formats = |list: &[(u32, &str)]| -> Vec<Value> {
        list.iter().map(|(code, label)| json!({"code": code, "picture": diacraft_calc::field_picture(*code), "label": label})).collect()
    };
    Ok(json!({
        "page": page,
        "categories": categories,
        "formats": {
            "date": formats(&fields::DATE_FORMATS),
            "number": formats(&fields::NUMBER_FORMATS),
            "text": formats(&fields::TEXT_FORMATS),
        },
    }))
}

/// Works the fields' texts out again → `{updated, examined, skipped}`: the fields whose text changed, the fields looked at, and the fields of shapes that hold more than 1024 and were left.
fn update_fields(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.update_fields";
    let id = optional_whole(p, "id", ID)?;
    let named = optional_whole(p, "page", ID)?;
    let env = env_of(s);
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match id {
        Some(id) => Some(page_for(doc, named, &[id], ID)?),
        None => named,
    };
    let ids: Vec<u32> = id.into_iter().collect();
    let done = s.edit_if("Update fields", |doc| {
        let scope = match (page, ids.as_slice()) {
            (Some(page), [_, ..]) => Scope::Shapes(page, &ids),
            (Some(page), []) => Scope::Page(page),
            (None, _) => Scope::All,
        };
        let was = doc.clone();
        let done = fields::refresh(doc, scope, &env, true).map_err(|e| e.into_engine(ID))?;
        // A field that now shows a longer text widens a shape sized to it: the connectors glued to it follow.
        draw::route_recalculated_pages(doc, &was).map_err(|e| e.into_engine(ID))?;
        Ok((done, done.updated > 0))
    })?;
    Ok(json!({"updated": done.updated, "examined": done.examined, "skipped": done.skipped}))
}

/// Puts characters in a shape's text → `{page, id}`.
fn insert_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.insert";
    let id = required_u32(p, "id", ID)?;
    let text = required_text(p, "text", ID)?;
    let at = place_of(p, ID)?;
    let page = target_page(s, p, id, ID)?;
    s.edit_if("Insert text", |doc| {
        let at = match at {
            Some(at) => at,
            // The end of the text the shape shows: its own, or its master's when it holds none.
            None => draw::shown_text_view(doc, page, id).map_or(0, |v| v.chars.chars().count()),
        };
        let changed = draw::set_text_range(doc, page, id, at, at, text).map_err(|e| e.into_engine(ID))?;
        Ok(((), changed))
    })?;
    Ok(json!({"page": page, "id": id}))
}

/// A picture from a file → `{page, id, format, width, height, drawn, shared, bytes, note?}`: the size in inches, whether the renderer draws the format (a GIF and a TIFF are kept and
/// drawn as a placeholder, and `note` says so) and whether the drawing held the same bytes already.
fn insert_picture(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "insert.picture";
    let path = required_text(p, "path", ID)?;
    if path.is_empty() {
        return Err(bad(ID, "`path` must name the picture file"));
    }
    let (x, y) = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    let centre = match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        (None, None) => None,
        _ => return Err(bad(ID, "`x` and `y` go together: give both or neither")),
    };
    let place = Where { centre, width: optional_number(p, "width", ID)?, height: optional_number(p, "height", ID)? };
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match optional_whole(p, "page", ID)? {
        Some(page) => page,
        None => doc.first_foreground_page().map(|pg| pg.id).ok_or_else(|| bad(ID, "the document has no pages"))?,
    };
    // The size is looked at before a byte is read: a file past the most a picture may be is refused by its length.
    let path = std::path::Path::new(path);
    let len = crate::file::file_len(path).map_err(|e| file_problem(ID, e))?;
    if len > pictures::MAX_PICTURE_BYTES {
        return Err(bad(
            ID,
            format!(
                "the picture's file is {} MiB, and a picture may be at most {} MiB",
                len.div_ceil(1024 * 1024),
                pictures::MAX_PICTURE_BYTES / (1024 * 1024)
            ),
        ));
    }
    let bytes = crate::file::read_file(path, pictures::MAX_PICTURE_BYTES).map_err(|e| file_problem(ID, e))?;
    let info = pictures::inspect(&bytes).map_err(|e| e.into_engine(ID))?;
    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(bytes);
    let size = shared.len();
    let above = s.id_floor(page);
    let made = s.edit("Insert picture", |doc| pictures::insert_picture(doc, page, &info, shared, &place, above).map_err(|e| e.into_engine(ID)))?;
    s.note_id(page, made.id);
    let mut out = json!({
        "page": page,
        "id": made.id,
        "format": info.format.word(),
        "width": made.width,
        "height": made.height,
        "drawn": info.format.drawn(),
        "shared": made.shared,
        "bytes": size,
    });
    if let (false, Some(map)) = (info.format.drawn(), out.as_object_mut()) {
        map.insert(
            "note".to_owned(),
            json!(format!(
                "a {} is kept in the drawing as it is and shown as a placeholder: DiaCraft does not decode that format yet",
                info.format.word().to_ascii_uppercase()
            )),
        );
    }
    Ok(out)
}

/// The refusal for a file that was not read.
fn file_problem(cmd: &str, error: crate::file::FileError) -> crate::error::EngineError {
    match error {
        crate::file::FileError::NotFound | crate::file::FileError::NotAFile => bad(cmd, error.to_string()),
        other => failed(cmd, other.to_string()),
    }
}
