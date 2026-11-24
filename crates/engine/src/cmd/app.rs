//! Application-level commands.

use serde_json::{Value, json};

use super::{CommandSpec, always};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(query "app.version", "Version", [], None, "none", always, version)]
}

/// → `{app, version, git, year}`: the application's version, the build line (`0.2.0` for a build of its tag, `0.2.0-dev+aab1664d` before the tag exists, or `unknown`) and the year of that
/// commit. The last two are what the application handed the engine at its start (`build_info`); until it does, `unknown` and `2026`.
fn version(_: &mut Session, _: &Value) -> Result<Value> {
    let build = crate::build_info::get();
    Ok(json!({ "app": "diacraft", "version": env!("CARGO_PKG_VERSION"), "git": build.git, "year": build.year }))
}
