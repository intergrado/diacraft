//! Documents and sessions in known states.
// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on
// panicking (AGENTS.md, Never crash) is for code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

/// A blank document: what `file.new` makes.
pub fn blank_document() -> Document {
    Document::new_blank()
}

/// A session whose document is blank: `file.new` has run, so it has one page, no undo steps and a
/// one-entry journal.
pub fn blank_session() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).expect("file.new works on a new session");
    session
}
