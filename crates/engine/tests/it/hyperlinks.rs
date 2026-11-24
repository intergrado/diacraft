//! The hyperlinks of a shape (`hyperlink.set`, `hyperlink.list`, `hyperlink.follow`), through `Session::execute` as the control channel, the CLI and MCP call
//! them. The rules are the specs': a link is a named row of the shape's `Hyperlink` section (`Address`, `SubAddress`, `Description`, `Default`, `NewWindow`,
//! `Invisible`), and say how it is made and followed. The decisions the specs leave open are in `docs/editing-commands.md`, "Hyperlinks".

use diacraft_doc::{Document, Owner, RowKey, SectionKey};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// The names of the rows of the `Hyperlink` section of shape `id` on page 1, in order.
fn row_names(d: &Document, id: u32) -> Vec<String> {
    let at = d.find_shape(Owner::Page(1), id).expect("the shape");
    let shape = d.shape(&at).expect("the shape");
    shape
        .sheet
        .section(&SectionKey::new("Hyperlink", None))
        .map(|s| s.rows.iter().filter_map(|r| r.name.as_ref().map(|n| n.to_string())).collect())
        .unwrap_or_default()
}

/// What a cell of a row of the `Hyperlink` section holds, as text.
fn cell(d: &Document, id: u32, row: &str, name: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let shape = d.shape(&at)?;
    let section = shape.sheet.section(&SectionKey::new("Hyperlink", None))?;
    let row = section.row(&RowKey::name(row))?;
    row.cells.get(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// Changes the top-level shape `id` of page 1 in the model itself, past the commands' checks, as a file can.
pub(crate) fn edit_shape(s: &mut Session, id: u32, f: impl FnOnce(&mut diacraft_doc::Shape)) {
    s.edit("test: edit a shape", |d| {
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at.path[0] as usize).unwrap());
        f(shape);
        Ok(())
    })
    .unwrap();
}

/// Gives shape `id` one link whose address and sub-address are exactly these, as a file can hold them (no command's check was made).
fn raw_link(s: &mut Session, id: u32, address: &str, sub_address: &str) {
    use diacraft_doc::{Cell, Row, Section, Value};
    use std::sync::Arc;
    edit_shape(s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|sec| sec.name.as_str() != "Hyperlink");
        let mut section = Section::new("Hyperlink", None);
        let mut row = Row { name: Some(Arc::from("Row_1")), ..Row::default() };
        row.cells.insert("Address".into(), Cell::constant(Value::from_text(address)));
        row.cells.insert("SubAddress".into(), Cell::constant(Value::from_text(sub_address)));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    });
}

fn links_of(s: &mut Session, id: u32) -> Vec<Json> {
    run(s, "hyperlink.list", json!({"id": id}))["links"].as_array().unwrap().clone()
}

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    match &e {
        diacraft_engine::EngineError::BadParams { msg, .. } | diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

#[test]
fn a_shape_has_no_hyperlink_until_one_is_set() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    assert!(links_of(&mut s, id).is_empty());
    assert!(row_names(doc(&s), id).is_empty());
}

#[test]
fn a_hyperlink_is_a_named_row_of_the_shapes_hyperlink_section() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let made = run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"description": "Home page", "address": "https://example.org/start", "sub_address": ""}]}),
    );
    assert_eq!(made["id"], id);
    assert_eq!(row_names(doc(&s), id), ["Row_1"], "the first row is named as Row_1");
    assert_eq!(cell(doc(&s), id, "Row_1", "Address").as_deref(), Some("https://example.org/start"));
    assert_eq!(cell(doc(&s), id, "Row_1", "Description").as_deref(), Some("Home page"));
    assert_eq!(cell(doc(&s), id, "Row_1", "SubAddress").as_deref(), Some(""));
    let listed = links_of(&mut s, id);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["name"], "Row_1");
    assert_eq!(listed[0]["address"], "https://example.org/start");
    assert_eq!(listed[0]["description"], "Home page");
    assert_eq!(listed[0]["default"], false);
    assert_eq!(listed[0]["new_window"], false);
    assert_eq!(listed[0]["kind"], "web", "the link says what it points at");
    assert_eq!(made["links"], json!(listed), "the answer of the set is the list the shape has now");
}

#[test]
fn the_set_is_one_undo_step_and_undo_takes_the_links_away() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let steps = s.history().unwrap().undo_len();
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"address": "https://a.example/"}, {"address": "mailto:me@example.org", "description": "Mail"}]}),
    );
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "two links, one step");
    assert_eq!(row_names(doc(&s), id), ["Row_1", "Row_2"]);
    run(&mut s, "edit.undo", json!({}));
    assert!(row_names(doc(&s), id).is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(row_names(doc(&s), id), ["Row_1", "Row_2"]);
}

#[test]
fn a_set_keeps_the_rows_it_names_changes_them_adds_new_ones_after_and_takes_away_the_rest() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"address": "https://a.example/"}, {"address": "https://b.example/"}, {"address": "https://c.example/"}]}),
    );
    // Keep Row_3 (changed), take Row_1 and Row_2 away, add one.
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"name": "Row_3", "address": "https://c.example/new", "description": "C"}, {"address": "https://d.example/"}]}),
    );
    assert_eq!(row_names(doc(&s), id), ["Row_3", "Row_4"], "Row_3 kept, the new row takes the first number no row has had");
    assert_eq!(cell(doc(&s), id, "Row_3", "Address").as_deref(), Some("https://c.example/new"));
    assert_eq!(cell(doc(&s), id, "Row_4", "Address").as_deref(), Some("https://d.example/"));
    // An empty list takes them all away.
    run(&mut s, "hyperlink.set", json!({"id": id, "links": []}));
    assert!(links_of(&mut s, id).is_empty());
}

#[test]
fn a_set_that_changes_nothing_is_no_undo_step() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "hyperlink.set", json!({"id": id, "links": [{"address": "https://a.example/", "description": "A"}]}));
    let steps = s.history().unwrap().undo_len();
    let revision = s.revision();
    run(&mut s, "hyperlink.set", json!({"id": id, "links": [{"name": "Row_1", "address": "https://a.example/", "description": "A"}]}));
    assert_eq!(s.history().unwrap().undo_len(), steps);
    assert_eq!(s.revision(), revision);
}

#[test]
fn one_link_is_the_default_and_a_second_default_is_refused() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"address": "https://a.example/"}, {"address": "https://b.example/", "default": true, "new_window": true}]}),
    );
    let listed = links_of(&mut s, id);
    assert_eq!(listed[0]["default"], false);
    assert_eq!(listed[1]["default"], true);
    assert_eq!(listed[1]["new_window"], true);
    let before = snapshot(&s);
    let words = refused(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"address": "https://a.example/", "default": true}, {"address": "https://b.example/", "default": true}]}),
    );
    assert!(words.contains("default"), "{words}");
    assert_eq!(snapshot(&s), before, "a refusal changes nothing");
}

#[test]
fn a_link_needs_an_address_or_a_sub_address_and_its_text_is_checked() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = snapshot(&s);
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": [{"description": "nothing to go to"}]}));
    assert!(words.contains("address"), "{words}");
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": [{"address": "https://a.example/\u{7}bell"}]}));
    assert!(words.contains("control"), "{words}");
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": [{"address": "x".repeat(5_000)}]}));
    assert!(words.contains("at most"), "{words}");
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": [{"address": "https://a.example/", "colour": "red"}]}));
    assert!(words.contains("colour"), "an unknown key is named: {words}");
    let many: Vec<Json> = (0..300).map(|n| json!({"address": format!("https://a.example/{n}")})).collect();
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": many}));
    assert!(words.contains("at most"), "{words}");
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": [{"name": "Row_9", "address": "https://a.example/"}]}));
    assert!(words.contains("Row_9"), "a row the shape does not have is named: {words}");
    assert_eq!(snapshot(&s), before, "every refusal left the drawing as it was");
    check_session(&s).unwrap();
}

#[test]
fn a_shape_that_is_not_there_is_refused_in_words() {
    let mut s = blank_session();
    let words = refused(&mut s, "hyperlink.set", json!({"id": 999, "links": []}));
    assert!(words.contains("999"), "{words}");
    let words = refused(&mut s, "hyperlink.list", json!({"id": 999}));
    assert!(words.contains("999"), "{words}");
}

#[test]
fn a_link_with_only_a_sub_address_goes_to_a_page_of_the_drawing() {
    // An empty Address and a Sub-address naming page 2 go to page 2.
    let mut s = blank_session();
    let second = run(&mut s, "page.add", json!({"name": "Second"}))["id"].as_u64().unwrap();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "hyperlink.set", json!({"id": id, "links": [{"sub_address": "Second"}]}));
    let go = run(&mut s, "hyperlink.follow", json!({"id": id}));
    assert_eq!(go["kind"], "page");
    assert_eq!(go["target_page"], second);
    assert!(go.get("target_shape").is_none_or(Json::is_null));
    assert_eq!(links_of(&mut s, id)[0]["kind"], "page");
}

#[test]
fn a_sub_address_after_a_slash_names_a_shape_of_that_page_by_its_name_or_its_number() {
    let mut s = blank_session();
    let target = drop_shape(&mut s, "rectangle", 5.0, 5.0);
    let page_name = run(&mut s, "document.inspect", json!({}))["pages"][0]["name"].as_str().unwrap().to_owned();
    // The shape's name is its own `Name`: set in the model, since no command names a shape yet.
    let shape_name = "Target box";
    edit_shape(&mut s, target, |shape| shape.name = Some(std::sync::Arc::from(shape_name)));
    let from = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    run(&mut s, "hyperlink.set", json!({"id": from, "links": [{"sub_address": format!("{page_name}/{shape_name}")}]}));
    let go = run(&mut s, "hyperlink.follow", json!({"id": from}));
    assert_eq!((go["kind"].as_str(), go["target_page"].as_u64(), go["target_shape"].as_u64()), (Some("shape"), Some(1), Some(u64::from(target))));
    // By the shape's number: `Sheet.<id>` as a formula names it.
    run(&mut s, "hyperlink.set", json!({"id": from, "links": [{"name": "Row_1", "sub_address": format!("{page_name}/Sheet.{target}")}]}));
    let go = run(&mut s, "hyperlink.follow", json!({"id": from}));
    assert_eq!(go["target_shape"].as_u64(), Some(u64::from(target)));
    // A shape or a page that is not there is refused, naming the words that were not found.
    run(&mut s, "hyperlink.set", json!({"id": from, "links": [{"name": "Row_1", "sub_address": "No such page"}]}));
    let words = refused(&mut s, "hyperlink.follow", json!({"id": from}));
    assert!(words.contains("No such page"), "{words}");
}

#[test]
fn a_web_or_mail_address_is_followed_as_the_address_and_nothing_else_is() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let cases = [
        ("https://example.org/a", "", "web", "https://example.org/a"),
        ("http://example.org/a b", "", "web", "http://example.org/a%20b"),
        ("https://example.org/a", "part", "web", "https://example.org/a#part"),
        ("ftp://files.example.org/pub", "", "web", "ftp://files.example.org/pub"),
        ("mailto:me@example.org", "", "mail", "mailto:me@example.org"),
        ("HTTPS://Example.org/Up", "", "web", "HTTPS://Example.org/Up"),
    ];
    for (n, (address, sub, kind, url)) in cases.iter().enumerate() {
        let name = format!("Row_{}", n + 1);
        let mut links: Vec<Json> =
            (0..n).map(|k| json!({"name": format!("Row_{}", k + 1), "address": cases[k].0, "sub_address": cases[k].1})).collect();
        links.push(json!({"address": address, "sub_address": sub}));
        run(&mut s, "hyperlink.set", json!({"id": id, "links": links}));
        let go = run(&mut s, "hyperlink.follow", json!({"id": id, "link": name}));
        assert_eq!(go["kind"], *kind, "{address}");
        assert_eq!(go["url"], *url, "{address}");
    }
    // What a click must not do: open a file or a program, or run a script. Each is refused in words and nothing is followed. The addresses are put in as a file holds them.
    for (address, words_have) in [
        ("file:///usr/bin/something", "web"),
        ("javascript:alert(1)", "web"),
        ("ms-msdt:/id", "web"),
        ("data:text/html,x", "web"),
        ("C:\\Windows\\system32\\calc.exe", "file"),
        ("../other.vsdx", "file"),
        ("https://", "address"),
        ("http://exa\u{202e}mple.org", "control"),
        ("https://exa\u{7}mple.org", "control"),
    ] {
        raw_link(&mut s, id, address, "");
        let words = refused(&mut s, "hyperlink.follow", json!({"id": id}));
        assert!(words.contains(words_have), "{address}: {words}");
        // The list says what the address is, too, without following it.
        let kind = links_of(&mut s, id)[0]["kind"].as_str().unwrap().to_owned();
        assert!(["file", "other", "web", "mail"].contains(&kind.as_str()), "{address}: {kind}");
    }
}

/// The followed link of a shape holding exactly this address and sub-address, or the words it is refused in.
fn followed(s: &mut Session, id: u32, address: &str, sub_address: &str) -> Result<Json, String> {
    raw_link(s, id, address, sub_address);
    s.execute("hyperlink.follow", &json!({"id": id})).map_err(|e| match e {
        diacraft_engine::EngineError::Failed { msg, .. } | diacraft_engine::EngineError::BadParams { msg, .. } => msg,
        other => panic!("expected a refusal in words, got {other:?}"),
    })
}

#[test]
fn a_sub_address_is_held_to_the_rules_of_the_address_and_the_fragment_is_percent_encoded() {
    // The review's first finding: the sub-address was joined to the address after only the address was looked at, and only spaces were escaped.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for (sub, why) in [
        ("part\u{202e}evil", "a direction mark"),
        ("part\nnext", "a line break"),
        ("part\u{7}bell", "a control character"),
        ("part\u{200b}zero", "a zero-width mark"),
        ("part%0Anext", "a line break written as a percent sequence"),
        ("part%E2%80%AEevil", "a direction mark written as percent sequences"),
        ("part%00nul", "a nul written as a percent sequence"),
        ("part%ZZ", "a percent sign that starts no sequence"),
        ("part%FF", "bytes that are no text"),
        ("100%", "a percent sign at the end"),
    ] {
        let words = followed(&mut s, id, "https://example.org/a", sub).expect_err(why);
        assert!(words.contains("sub-address"), "{why}: {words}");
    }
    // A fragment is written with every character that is not safe as a percent sequence, a character of the text as its bytes.
    for (sub, url) in [
        ("a\"b<c>d", "https://example.org/a#a%22b%3Cc%3Ed"),
        ("a b", "https://example.org/a#a%20b"),
        ("`{|}^\\", "https://example.org/a#%60%7B%7C%7D%5E%5C"),
        ("caf\u{e9}", "https://example.org/a#caf%C3%A9"),
        ("a%41b", "https://example.org/a#a%41b"),
        ("sec-1.2_x~y/z?q=1&r=2:@!$'()*+,;", "https://example.org/a#sec-1.2_x~y/z?q=1&r=2:@!$'()*+,;"),
    ] {
        let go = followed(&mut s, id, "https://example.org/a", sub).unwrap_or_else(|w| panic!("{sub}: {w}"));
        assert_eq!(go["url"], url, "{sub}");
    }
    // The same marks in the address are refused in the address's words, also written as percent sequences.
    for address in ["https://example.org/%0d%0aSet-Cookie:x", "https://example.org/a%E2%80%AEb", "ftp://files.example.org/%00"] {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains("address"), "{address}: {words}");
    }
}

#[test]
fn a_mail_link_is_followed_with_recipients_a_subject_a_body_and_copies_and_nothing_else() {
    // The review's second finding: the whole mailto: URL went to the mail client, with parameters some clients honour to attach a local file.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for (address, url) in [
        ("mailto:a@b.example", "mailto:a@b.example"),
        ("mailto:a@b.example?subject=x&body=y", "mailto:a@b.example?subject=x&body=y"),
        ("mailto:a@b.example,c@d.example?cc=e@f.example&bcc=g@h.example", "mailto:a@b.example,c@d.example?cc=e@f.example&bcc=g@h.example"),
        ("MAILTO:a@b.example?SUBJECT=Hello%20there", "mailto:a@b.example?subject=Hello%20there"),
        ("mailto:a@b.example?subject=Hello there", "mailto:a@b.example?subject=Hello%20there"),
        ("mailto:?subject=x", "mailto:?subject=x"),
        ("mailto:a@b.example?body=a%26b", "mailto:a@b.example?body=a%26b"),
    ] {
        let go = followed(&mut s, id, address, "").unwrap_or_else(|w| panic!("{address}: {w}"));
        assert_eq!((go["kind"].as_str(), go["url"].as_str()), (Some("mail"), Some(url)), "{address}");
    }
    for (address, named) in [
        ("mailto:a@b.example?attach=/etc/passwd", "attach"),
        ("mailto:a@b.example?attachment=/etc/passwd", "attachment"),
        ("mailto:a@b.example?subject=x&Attach=%2Fetc%2Fpasswd", "Attach"),
        ("mailto:a@b.example?in-reply-to=x", "in-reply-to"),
        ("mailto:a@b.example?x-run=calc", "x-run"),
        ("mailto:a@b.example?subject", "subject"),
        ("mailto:a@b.example?subject=x&&body=y", "empty"),
    ] {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains(named), "{address}: {words}");
    }
    // Control or invisible characters, also written as percent sequences, are refused in a recipient and in each value.
    for address in [
        "mailto:a@b.example%0ABcc:x@y.example",
        "mailto:a@b.example?subject=x%0D%0ABcc:x@y.example",
        "mailto:a@b.example?body=%E2%80%AE",
        "mailto:a@b.example?subject=x\u{202e}",
        "mailto:a@b.example?subject=%",
        "mailto:a@b.example?cc=%FF",
        "mailto:\u{7}",
    ] {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains("mail"), "{address}: {words}");
    }
    // A mail link with nothing to send to is refused, as it was.
    let words = followed(&mut s, id, "mailto:", "").expect_err("nothing after mailto:");
    assert!(words.contains("mail"), "{words}");
}

#[test]
fn a_web_address_is_read_the_way_the_browser_reads_it_or_refused() {
    // The review's third round, first finding: an address whose percent sequences did not decode was checked as it was written, and the browser decodes it another way.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for address in [
        "https://example.org/100%",
        "https://example.org/%ZZ",
        "https://example.org/%",
        "https://example.org/%4",
        "https://example.org/%FF",
        "https://example.org/caf%E9",
        "https://example.org/a%C3",
        "ftp://files.example.org/%E2%80",
        "https://example.org/?q=%%0a",
        "https://example.org/%E2%80%AEevil",
        "https://example.org/a%00b",
        "https://example.org/a%7Fb",
        "https://example.org/a%C2%85b",
        "https://example.org/%EF%BB%BF",
    ] {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains("address") && words.contains("percent"), "{address}: {words}");
    }
    // What decodes is followed, as it was written.
    for (address, url) in [
        ("https://example.org/caf%C3%A9", "https://example.org/caf%C3%A9"),
        ("https://example.org/a%20b?q=%26x", "https://example.org/a%20b?q=%26x"),
        ("https://example.org/100%25", "https://example.org/100%25"),
    ] {
        let go = followed(&mut s, id, address, "").unwrap_or_else(|w| panic!("{address}: {w}"));
        assert_eq!(go["url"], url, "{address}");
    }
}

#[test]
fn a_recipient_cannot_make_a_query_the_parameter_list_never_saw() {
    // The review's third round, second finding: `?`, `&`, `=` and `,` stayed as they were in a recipient, so a recipient could re-make a query.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for (address, url) in [
        // A recipient that holds the marks written as percent sequences: written again as percent sequences, with no query.
        ("mailto:a@b.example%3Fattach=/etc/passwd", "mailto:a@b.example%3Fattach%3D/etc/passwd"),
        ("mailto:a@b.example%3Fattach%3D/etc/passwd", "mailto:a@b.example%3Fattach%3D/etc/passwd"),
        ("mailto:a%26b%23c%25d@x.example", "mailto:a%26b%23c%25d@x.example"),
        ("mailto:a%2Cb@x.example", "mailto:a%2Cb@x.example"),
        // A value too: a question mark in it is written as a percent sequence.
        ("mailto:a@b.example?subject=a%3Fattach=1", "mailto:a@b.example?subject=a%3Fattach%3D1"),
        ("mailto:a@b.example?subject=a?b", "mailto:a@b.example?subject=a%3Fb"),
        ("mailto:a@b.example?cc=c@d.example,e@f.example", "mailto:a@b.example?cc=c@d.example,e@f.example"),
    ] {
        let go = followed(&mut s, id, address, "").unwrap_or_else(|w| panic!("{address}: {w}"));
        let got = go["url"].as_str().unwrap();
        assert_eq!(got, url, "{address}");
        // Whatever came in, the URL holds at most the question mark that starts the parameters the list checked.
        let queries = got.matches('?').count();
        assert_eq!(queries, usize::from(url.contains('?')), "{address}: {got}");
    }
    // A raw question mark in a recipient is where the parameters start, so the parameter is read and refused.
    let words = followed(&mut s, id, "mailto:a@b.example?attach=/etc/passwd", "").expect_err("an attachment");
    assert!(words.contains("attach"), "{words}");
}

#[test]
fn every_scheme_a_click_does_not_follow_is_refused_in_words_and_each_one_that_is_followed_is_listed() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for address in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "vbscript:msgbox(1)",
        "data:text/html,<script>alert(1)</script>",
        "file:///usr/bin/xterm",
        "FILE://host/share/run.exe",
        "ms-msdt:/id",
        "ms-excel:ofe|u|https://example.org/x.xlsx",
        "tel:+15555550100",
        "sms:+15555550100",
        "steam://run/1",
        "smb://host/share",
        "ssh://host/",
        "ldap://host/",
        "view-source:https://example.org/",
        "blob:https://example.org/id",
        "about:blank",
    ] {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains("not followed") && words.contains("web"), "{address}: {words}");
    }
    // A path, a drive, a share and a relative name are files, and a click opens no file or program.
    for address in
        ["C:\\Windows\\system32\\calc.exe", "\\\\host\\share\\run.exe", "/usr/bin/xterm", "./run.sh", "../other.vsdx", "notes.txt", "run.exe", "~/x"]
    {
        let words = followed(&mut s, id, address, "").expect_err(address);
        assert!(words.contains("not followed") && words.contains("file"), "{address}: {words}");
    }
    // The four that are followed.
    for address in ["http://example.org/", "https://example.org/", "ftp://example.org/", "ftps://example.org/", "mailto:a@b.example"] {
        followed(&mut s, id, address, "").unwrap_or_else(|w| panic!("{address}: {w}"));
    }
}

#[test]
fn the_link_followed_is_the_default_one_else_the_first_that_is_not_hidden() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [
            {"address": "https://hidden.example/", "invisible": true},
            {"address": "https://first.example/"},
            {"address": "https://default.example/", "default": true}
        ]}),
    );
    assert_eq!(run(&mut s, "hyperlink.follow", json!({"id": id}))["url"], "https://default.example/");
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": id, "links": [{"name": "Row_1", "address": "https://hidden.example/", "invisible": true}, {"name": "Row_2", "address": "https://first.example/"}]}),
    );
    assert_eq!(run(&mut s, "hyperlink.follow", json!({"id": id}))["url"], "https://first.example/");
    let words = refused(&mut s, "hyperlink.follow", json!({"id": id, "link": "Row_9"}));
    assert!(words.contains("Row_9"), "{words}");
    run(&mut s, "hyperlink.set", json!({"id": id, "links": []}));
    let words = refused(&mut s, "hyperlink.follow", json!({"id": id}));
    assert!(words.contains("no hyperlink"), "{words}");
}

#[test]
fn a_hostile_section_of_a_hundred_thousand_rows_is_read_to_a_cap_and_said() {
    use diacraft_doc::{Cell, Row, Section, Value};
    use std::sync::Arc;
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    // A file can hold a section of any number of rows: this one is built in the model and read through the command.
    edit_shape(&mut s, id, |shape| {
        let mut section = Section::new("Hyperlink", None);
        for n in 0..100_000 {
            let mut row = Row { name: Some(Arc::from(format!("Row_{n}"))), ..Row::default() };
            row.cells.insert("Address".into(), Cell::constant(Value::from_text("https://a.example/")));
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    let words = refused(&mut s, "hyperlink.list", json!({"id": id}));
    assert!(words.contains("100000") || words.contains("100,000"), "the count is said: {words}");
    assert!(words.contains("at most"), "{words}");
    let words = refused(&mut s, "hyperlink.set", json!({"id": id, "links": []}));
    assert!(words.contains("at most"), "{words}");
    let words = refused(&mut s, "hyperlink.follow", json!({"id": id}));
    assert!(words.contains("at most"), "{words}");
}

#[test]
fn junk_parameters_are_refused_in_words() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for junk in [
        json!("x"),
        json!(null),
        json!(7),
        json!([1]),
        json!({"address": 1}),
        json!([{"address": null}]),
        json!([[]]),
        json!([{"address": "https://a.example/", "default": "yes"}]),
    ] {
        let e = s.execute("hyperlink.set", &json!({"id": id, "links": junk})).expect_err("junk is refused");
        assert!(!bad_params_msg(&e).is_empty());
    }
    check_session(&s).unwrap();
}
