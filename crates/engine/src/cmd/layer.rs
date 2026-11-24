//! Layers: `layer.list`, `layer.add`, `layer.set`, `layer.assign` and `layer.delete`. The rules are in `draw/layers.rs` and `docs/editing-commands.md`
//! ("Layers"). A layer is asked for by its number (the
//! `IX` of its row, which a shape's list holds) or by its name.

use serde_json::{Map, Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::layers::{self, Mode, Settings, Which};
use crate::error::{Result, bad, failed};
use crate::formatting::Rgb;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "layer.list",
            "Layers",
            ["Home", "Editing", "Layers"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background)",
            has_document,
            list
        ),
        cmd!(
            "layer.add",
            "New Layer",
            ["Home", "Editing", "Layers"],
            None,
            "name: text, the layer's name: not empty, at most 255 characters, no control character, not another layer's of the page and not one of the five names the OpenDocument format reserves (layout, background, backgroundobjects, controls, measurelines); \
             page: whole number, optional, the id of the page (default: the first page that is not a background); \
             visible: true or false, optional (default true); print: true or false, optional (default true); active: true or false, optional (default false, and a locked layer cannot be active); \
             lock: true or false, optional (default false); snap: true or false, optional (default true); glue: true or false, optional (default true); \
             color: text, optional, the layer's colour as #rrggbb, or none (default none), laid over the shapes that are on this layer alone; transparency: number, optional, from 0 (opaque, the default) to 1 (clear)",
            has_document,
            add
        ),
        cmd!(
            "layer.set",
            "Layer Properties",
            ["Home", "Editing", "Layers"],
            None,
            "layer: a whole number (the layer's number) or text (its name), the layer to change; page: whole number, optional, the id of the page (default: the first page that is not a background); \
             name: text, optional, the new name (the shapes stay on the layer); visible: true or false, optional, show or hide the layer; print: true or false, optional; \
             active: true or false, optional, new shapes join the layer (refused for a locked layer); lock: true or false, optional; snap: true or false, optional; glue: true or false, optional; \
             color: text, optional, #rrggbb or none; transparency: number, optional, from 0 to 1. Locking an active layer makes it inactive, and the answer says so",
            has_document,
            set
        ),
        cmd!(
            "layer.assign",
            "Assign to Layer",
            ["Home", "Editing", "Layers"],
            None,
            "ids: array of whole numbers, the shapes; layers: array of layers, each a whole number (the layer's number) or text (its name), empty for none; \
             mode: text, optional, replace (the default: the shape is on these layers and no others), add or remove; \
             preserve_members: true or false, optional (default true), false puts a group's layers on its members too, at any depth; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            assign
        ),
        cmd!(
            "layer.delete",
            "Remove Layer",
            ["Home", "Editing", "Layers"],
            None,
            "layer: a whole number (the layer's number) or text (its name), the layer to remove; unreferenced: true to remove every layer no shape is on, instead of one; \
             shapes: text, optional, keep (the default: the shapes that were on the layer stay, on no layer if it was their only one) or delete (the shapes that are on none but the removed layers are deleted); \
             page: whole number, optional, the id of the page (default: the first page that is not a background). The layers after a removed one move up, and the shapes' layer numbers follow",
            has_document,
            delete
        ),
    ]
}

/// The page a command works on: the one named, else the first page that is not a background, else the first.
fn page_of(s: &Session, p: &Value, cmd: &str) -> Result<u32> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    match optional_whole(p, "page", cmd)? {
        Some(page) => Ok(page),
        None => doc
            .pages
            .iter()
            .find(|page| !page.background)
            .or_else(|| doc.pages.iter().next())
            .map(|page| page.id)
            .ok_or_else(|| bad(cmd, "the document has no pages")),
    }
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// `#rrggbb`, or `none` for 255.
fn colour(p: &Value, cmd: &str) -> Result<Option<Option<(u8, u8, u8)>>> {
    match p.get("color") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.trim().eq_ignore_ascii_case("none") => Ok(Some(None)),
        Some(Value::String(text)) => match Rgb::parse(text.trim()) {
            Some(c) => Ok(Some(Some((c.0, c.1, c.2)))),
            None => Err(bad(cmd, "`color` must be a colour written #rrggbb, or none")),
        },
        Some(_) => Err(bad(cmd, "`color` must be a colour written #rrggbb, or none")),
    }
}

fn settings_of(p: &Value, cmd: &str) -> Result<Settings> {
    Ok(Settings {
        name: optional_text(p, "name", cmd)?.map(str::to_owned),
        visible: flag(p, "visible", cmd)?,
        print: flag(p, "print", cmd)?,
        active: flag(p, "active", cmd)?,
        lock: flag(p, "lock", cmd)?,
        snap: flag(p, "snap", cmd)?,
        glue: flag(p, "glue", cmd)?,
        colour: colour(p, cmd)?,
        transparency: optional_number(p, "transparency", cmd)?,
    })
}

fn which(value: &Value, key: &str, cmd: &str) -> Result<Which> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Which::Ix)
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a layer's number or its name"))),
        Value::String(text) => Ok(Which::Name(text.clone())),
        _ => Err(bad(cmd, format!("`{key}` must be a layer's number or its name"))),
    }
}

fn layer_json(l: &layers::Layer) -> Value {
    let mut out = Map::new();
    out.insert("id".to_owned(), json!(l.ix));
    out.insert("name".to_owned(), json!(l.name));
    out.insert("name_univ".to_owned(), json!(l.name_univ));
    for (key, on) in [("visible", l.visible), ("print", l.print), ("active", l.active), ("lock", l.lock), ("snap", l.snap), ("glue", l.glue)] {
        out.insert(key.to_owned(), Value::Bool(on));
    }
    out.insert("color".to_owned(), l.colour.map_or(Value::Null, |(r, g, b)| json!(format!("#{r:02x}{g:02x}{b:02x}"))));
    out.insert("transparency".to_owned(), json!(l.transparency));
    out.insert("shapes".to_owned(), json!(l.shapes));
    Value::Object(out)
}

/// The layers of a page, with how many shapes are on each → `{page, layers}`.
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.list";
    let page = page_of(s, p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let all = layers::page_layers(doc, page).map_err(|e| e.into_engine(ID))?;
    Ok(json!({ "page": page, "layers": all.iter().map(layer_json).collect::<Vec<_>>() }))
}

/// Adds a layer → `{page, id, name}`.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.add";
    let page = page_of(s, p, ID)?;
    let settings = settings_of(p, ID)?;
    if settings.name.is_none() {
        return Err(bad(ID, "missing `name`"));
    }
    s.edit("Add layer", |doc| {
        let ix = layers::add_layer(doc, page, &settings).map_err(|e| e.into_engine(ID))?;
        let name =
            layers::read_layers(doc, page).map_err(|e| e.into_engine(ID))?.into_iter().find(|l| l.ix == ix).map(|l| l.name).unwrap_or_default();
        Ok(json!({ "page": page, "id": ix, "name": name }))
    })
}

/// Changes a layer → `{page, id, name, deactivated}`.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.set";
    let page = page_of(s, p, ID)?;
    let which = which(p.get("layer").ok_or_else(|| bad(ID, "missing `layer`"))?, "layer", ID)?;
    let settings = settings_of(p, ID)?;
    s.edit("Change layer", |doc| {
        let table = layers::read_layers(doc, page).map_err(|e| e.into_engine(ID))?;
        let ix = layers::find(&table, &which).map_err(|e| e.into_engine(ID))?.ix;
        let outcome = layers::set_layer(doc, page, ix, &settings).map_err(|e| e.into_engine(ID))?;
        let name =
            layers::read_layers(doc, page).map_err(|e| e.into_engine(ID))?.into_iter().find(|l| l.ix == ix).map(|l| l.name).unwrap_or_default();
        Ok(json!({ "page": page, "id": ix, "name": name, "deactivated": outcome.deactivated }))
    })
}

/// Puts shapes on layers → `{page, shapes, layers, written}`.
fn assign(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.assign";
    let ids = required_ids(p, "ids", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, optional_whole(p, "page", ID)?, &ids, ID)?;
    let Some(Value::Array(items)) = p.get("layers") else {
        return Err(bad(ID, "`layers` must be an array of layers, each a number or a name (empty for none)"));
    };
    if items.len() > layers::MAX_LAYERS {
        return Err(bad(ID, format!("`layers` holds at most {} layers", layers::MAX_LAYERS)));
    }
    let named: Vec<Which> = items.iter().map(|v| which(v, "layers", ID)).collect::<Result<_>>()?;
    let mode = match optional_text(p, "mode", ID)? {
        None | Some("replace") => Mode::Replace,
        Some("add") => Mode::Add,
        Some("remove") => Mode::Remove,
        Some(_) => return Err(bad(ID, "`mode` must be replace, add or remove")),
    };
    let preserve = flag(p, "preserve_members", ID)?.unwrap_or(true);
    s.edit_if("Assign to layer", |doc| {
        let table = layers::read_layers(doc, page).map_err(|e| e.into_engine(ID))?;
        let mut numbers: Vec<u32> = Vec::with_capacity(named.len());
        for w in &named {
            numbers.push(layers::find(&table, w).map_err(|e| e.into_engine(ID))?.ix);
        }
        let done = layers::assign_layers(doc, page, &ids, &numbers, mode, preserve).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "shapes": done.shapes, "layers": numbers, "written": done.written }), done.written > 0))
    })
}

/// Removes layers → `{page, removed, shapes_deleted}`.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.delete";
    let page = page_of(s, p, ID)?;
    let delete_shapes = match optional_text(p, "shapes", ID)? {
        None | Some("keep") => false,
        Some("delete") => true,
        Some(_) => return Err(bad(ID, "`shapes` must be keep or delete")),
    };
    let unreferenced = flag(p, "unreferenced", ID)?.unwrap_or(false);
    let one = match p.get("layer") {
        None | Some(Value::Null) => None,
        Some(v) => Some(which(v, "layer", ID)?),
    };
    if one.is_some() == unreferenced {
        return Err(bad(ID, "give `layer` (the layer to remove) or `unreferenced: true` (every layer no shape is on), and not both"));
    }
    s.edit_if("Remove layer", |doc| {
        let numbers: Vec<u32> = match &one {
            Some(w) => {
                let table = layers::read_layers(doc, page).map_err(|e| e.into_engine(ID))?;
                vec![layers::find(&table, w).map_err(|e| e.into_engine(ID))?.ix]
            }
            None => layers::unreferenced(doc, page).map_err(|e| e.into_engine(ID))?,
        };
        let done = layers::delete_layers(doc, page, &numbers, delete_shapes).map_err(|e| e.into_engine(ID))?;
        let changed = !done.removed.is_empty();
        Ok((json!({ "page": page, "removed": done.removed, "shapes_deleted": done.shapes_deleted }), changed))
    })
}
