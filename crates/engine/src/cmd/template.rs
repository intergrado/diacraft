//! Template commands: the list File > New shows. A drawing is started from a template by `file.new {template}` (`cmd/file.rs`); the rules
//! are in `template/` and `docs/editing-commands.md`, "Templates".

use serde_json::{Value, json};

use super::{CommandSpec, always};
use crate::Session;
use crate::draw::paper::{Orientation, size_preset};
use crate::error::Result;
use crate::stencil;
use crate::template::{self, TemplateSpec};
use crate::theme;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(query "template.list", "Templates", ["File", "New"], None, "none", always, list)]
}

/// The template as File > New shows it: its names, its page, its theme and the stencils it opens.
fn template_json(t: &TemplateSpec) -> Value {
    let size = size_preset(t.size);
    // The preset is portrait, in paper inches; the page is turned to the template's orientation.
    let (short, long) = size.map_or((0.0, 0.0), |s| (s.width, s.height));
    let (width, height) = if t.orientation == Orientation::Landscape { (long, short) } else { (short, long) };
    let theme = t.theme.and_then(|(id, variant)| theme::find(id).map(|th| json!({ "id": th.id, "name": th.name, "variant": variant })));
    let stencils: Vec<Value> =
        t.stencils.iter().filter_map(|id| stencil::builtin_stencil(id)).map(|s| json!({ "id": s.id, "name": s.name })).collect();
    json!({
        "id": t.id,
        "name": t.name,
        "category": t.category,
        "description": t.description,
        "page": {
            "size": t.size,
            "size_name": size.map(|s| s.name),
            "orientation": t.orientation.name(),
            "width": width,
            "height": height,
        },
        "theme": theme,
        "stencils": stencils,
    })
}

/// → `{templates: [{id, name, category, description, page: {size, size_name, orientation, width, height}, theme: {id, name, variant} | null,
/// stencils: [{id, name}]}], categories}`: the templates DiaCraft ships, in the order File > New lists them, with the page each starts (its size and
/// the page's width and height in inches), the theme it applies and the built-in stencils it opens, first shown first. `categories` is the distinct
/// categories, in the order they first appear. It needs no document. There is no thumbnail: a template's page is empty, so the window draws its card
/// from `page`, `theme` and `stencils`.
fn list(_: &mut Session, _: &Value) -> Result<Value> {
    let templates = template::templates();
    let mut categories: Vec<&str> = Vec::new();
    for t in templates {
        if !categories.contains(&t.category) {
            categories.push(t.category);
        }
    }
    Ok(json!({ "templates": templates.iter().map(template_json).collect::<Vec<_>>(), "categories": categories }))
}
