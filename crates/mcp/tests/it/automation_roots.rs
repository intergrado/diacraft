//! The MCP server kept to granted folders (gaps A2 and A5): `diacraft-cli mcp --headless --automation-read-root DIR --automation-write-root DIR` runs the server loop with the roots
//! in force, and every tool that reaches a file (`command_run` and `command_batch` of a command that reads or writes one) is kept to them. The tests run the loop the way the
//! command line does, with the roots in force on its thread, and look at what a client reads: the tool result, in words, with `isError`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::Session;
use diacraft_engine::file_access::{self, AutomationRoots};
use diacraft_mcp::Backend;
use serde_json::{Value, json};

use crate::{call, lines, request, talk_on, tool_text};

/// `read`, `write` and `outside`, each with a drawing, a stencil, a template and a picture, and the roots over the first two.
struct World {
    base: PathBuf,
    roots: Arc<AutomationRoots>,
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&[160u8; 8 * 8 * 4]).unwrap();
    }
    bytes
}

impl World {
    fn new() -> World {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!("diacraft-mcp-roots-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&base);
        for folder in ["read", "write", "outside"] {
            std::fs::create_dir_all(base.join(folder)).unwrap();
        }
        let roots = AutomationRoots::new(&[base.join("read").as_path()], &[base.join("write").as_path()]).unwrap().unwrap();
        let world = World { base, roots };
        for folder in ["read", "write", "outside"] {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            s.execute("file.save_as", &json!({"path": world.p(&format!("{folder}/drawing.diacraft"))})).unwrap();
            s.execute("file.save_as", &json!({"path": world.p(&format!("{folder}/template.diacraft-template"))})).unwrap();
            // A stencil with no masters is a file that opens: DiaCraft's own type, saved from a blank drawing.
            let mut stencil = Session::new();
            stencil.execute("file.new", &json!({})).unwrap();
            stencil.execute("file.save_as", &json!({"path": world.p(&format!("{folder}/stencil.diacraft-stencil"))})).unwrap();
            std::fs::write(world.base.join(folder).join("pic.png"), png()).unwrap();
        }
        world
    }

    fn p(&self, relative: &str) -> String {
        self.base.join(relative).to_str().unwrap().to_owned()
    }

    fn exists(&self, relative: &str) -> bool {
        self.base.join(relative).exists()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A headless server with the roots in force, one request at a time on the same session.
struct Server<'w> {
    world: &'w World,
    backend: Backend,
    next: u64,
}

impl<'w> Server<'w> {
    fn new(world: &'w World) -> Server<'w> {
        Server { world, backend: Backend::Headless(Box::new(Session::new())), next: 1 }
    }

    /// One message through the loop, with the roots in force; the reply.
    fn send(&mut self, message: &Value) -> Value {
        let input = lines(std::slice::from_ref(message));
        let replies = file_access::confine(Some(&self.world.roots), || talk_on(&mut self.backend, &input));
        replies.into_iter().next().unwrap()
    }

    /// A `command_run`: `Ok` is the command's text, `Err` the words of an `isError` result.
    fn run(&mut self, command: &str, params: Value) -> Result<String, String> {
        self.next += 1;
        let reply = self.send(&call(self.next, "command_run", json!({"id": command, "params": params})));
        let text = tool_text(&reply);
        if reply["result"]["isError"] == true { Err(text) } else { Ok(text) }
    }
}

/// A refusal in words: the rule and the flag, and nothing of the path.
fn rule(world: &World, words: &str, flag: &str) {
    assert!(words.contains("automation") && words.contains(flag), "{words}");
    assert!(!words.contains(&*world.base.to_string_lossy()), "holds the path: {words}");
}

#[test]
fn a_file_is_read_through_the_tools_only_inside_a_read_root() {
    let world = World::new();
    let mut server = Server::new(&world);
    assert!(server.run("file.open", json!({"path": world.p("read/drawing.diacraft")})).is_ok());
    for outside in ["outside/drawing.diacraft", "write/drawing.diacraft", "read/../outside/drawing.diacraft"] {
        let words = server.run("file.open", json!({"path": world.p(outside)})).unwrap_err();
        rule(&world, &words, "--automation-read-root");
    }
    assert!(server.run("file.new", json!({"template": world.p("read/template.diacraft-template")})).is_ok());
    rule(&world, &server.run("file.new", json!({"template": world.p("outside/template.diacraft-template")})).unwrap_err(), "--automation-read-root");
    server.run("file.new", json!({})).unwrap();
    assert!(server.run("stencil.open", json!({"path": world.p("read/stencil.diacraft-stencil")})).is_ok());
    rule(&world, &server.run("stencil.open", json!({"path": world.p("outside/stencil.diacraft-stencil")})).unwrap_err(), "--automation-read-root");
    assert!(server.run("insert.picture", json!({"path": world.p("read/pic.png")})).is_ok());
    rule(&world, &server.run("insert.picture", json!({"path": world.p("outside/pic.png")})).unwrap_err(), "--automation-read-root");
    let boss =
        serde_json::from_str::<Value>(&server.run("shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 4.25, "y": 9.5})).unwrap())
            .unwrap()["id"]
            .clone();
    assert!(server.run("orgchart.picture_set", json!({"id": boss, "path": world.p("read/pic.png")})).is_ok());
    rule(&world, &server.run("orgchart.picture_set", json!({"id": boss, "path": world.p("outside/pic.png")})).unwrap_err(), "--automation-read-root");
}

#[test]
fn a_file_is_written_through_the_tools_only_inside_a_write_root() {
    let world = World::new();
    let mut server = Server::new(&world);
    server.run("file.new", json!({})).unwrap();
    server.run("shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})).unwrap();
    assert!(server.run("file.save_as", json!({"path": world.p("write/a.vsdx")})).is_ok());
    assert!(world.exists("write/a.vsdx"));
    for outside in ["outside/b.vsdx", "read/b.vsdx", "write/../outside/b.vsdx"] {
        rule(&world, &server.run("file.save_as", json!({"path": world.p(outside)})).unwrap_err(), "--automation-write-root");
    }
    assert!(!world.exists("outside/b.vsdx") && !world.exists("read/b.vsdx"), "nothing was written");
    for (command, name) in [("file.export", "p.png"), ("file.export", "p.svg"), ("file.export", "p.pdf"), ("file.print", "print.pdf")] {
        assert!(server.run(command, json!({"path": world.p(&format!("write/{name}"))})).is_ok(), "{command} {name}");
        rule(&world, &server.run(command, json!({"path": world.p(&format!("outside/{name}"))})).unwrap_err(), "--automation-write-root");
        assert!(!world.exists(&format!("outside/{name}")), "{command} {name} wrote outside");
    }
    assert!(server.run("file.check_compatibility", json!({"path": world.p("write/c.vsdx")})).is_ok());
    rule(&world, &server.run("file.check_compatibility", json!({"path": world.p("outside/c.vsdx")})).unwrap_err(), "--automation-write-root");
    // `file.save` writes the drawing's own file, which is where `file.save_as` put it: inside a write root.
    server.run("page.add", json!({"name": "More"})).unwrap();
    assert!(server.run("file.save", json!({})).is_ok());
}

#[test]
fn file_save_is_refused_for_a_drawing_opened_from_a_read_root() {
    let world = World::new();
    let mut server = Server::new(&world);
    server.run("file.open", json!({"path": world.p("read/drawing.diacraft")})).unwrap();
    server.run("page.add", json!({"name": "Edited"})).unwrap();
    let before = std::fs::read(world.base.join("read/drawing.diacraft")).unwrap();
    rule(&world, &server.run("file.save", json!({})).unwrap_err(), "--automation-write-root");
    assert_eq!(std::fs::read(world.base.join("read/drawing.diacraft")).unwrap(), before, "the file is as it was");
}

#[test]
fn a_batch_with_a_refused_step_changes_nothing() {
    let world = World::new();
    let mut server = Server::new(&world);
    server.run("file.new", json!({})).unwrap();
    // `file.save_as` cannot run inside a batch at all, and `file.open` is one a batch does not take: the refusal that reaches the client is the batch's own, and no file is made.
    let reply = server.send(&call(
        7,
        "command_batch",
        json!({"steps": [{"id": "page.add", "params": {"name": "One"}}, {"id": "file.save_as", "params": {"path": world.p("outside/batch.diacraft")}}]}),
    ));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(!world.exists("outside/batch.diacraft"));
    let pages = server.send(&call(8, "doc_inspect", json!({})));
    assert!(!tool_text(&pages).contains("One"), "the first step was undone with the rest: {pages}");
}

#[test]
fn a_hostile_path_is_refused_in_words_and_the_server_goes_on() {
    let world = World::new();
    let mut server = Server::new(&world);
    let long = format!("{}/{}", world.p("read"), "a".repeat(64 * 1024));
    let words = server.run("file.open", json!({"path": long})).unwrap_err();
    assert!(words.contains("automation") && words.len() < 1000, "{} bytes of reply", words.len());
    // A long chain of `..`, under the system's own limit on a path, is taken by the system and lands where it lands: here, inside; one that leaves the root is outside.
    std::fs::create_dir_all(world.base.join("read/sub")).unwrap();
    let chain = format!("{}/{}drawing.diacraft", world.p("read"), "sub/../".repeat(500));
    match server.run("file.open", json!({"path": chain})) {
        Ok(_) => {}
        // A system whose own limit on a path is shorter refuses it, in the roots' words.
        Err(words) => assert!(words.contains("automation"), "{words}"),
    }
    let leaving = format!("{}/{}../outside/drawing.diacraft", world.p("read"), "sub/../".repeat(500));
    rule(&world, &server.run("file.open", json!({"path": leaving})).unwrap_err(), "--automation-read-root");
    let nul = format!("{}/a\0.diacraft", world.p("read"));
    assert!(server.run("file.open", json!({"path": nul})).unwrap_err().contains("automation"));
    assert!(server.run("file.open", json!({"path": ""})).is_err());
    // It still answers.
    assert!(server.run("file.open", json!({"path": world.p("read/drawing.diacraft")})).is_ok());
}

#[test]
fn the_server_tells_the_client_where_it_may_read_and_write() {
    let world = World::new();
    let mut server = Server::new(&world);
    let reply = server.send(&request(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    ));
    let words = reply["result"]["instructions"].as_str().unwrap();
    assert!(words.contains(&world.roots.read_roots()[0]) && words.contains(&world.roots.write_roots()[0]), "{words}");
    assert!(words.contains("read") && words.contains("write"), "{words}");
    // With no roots the server says its files are not limited.
    let mut free = Backend::Headless(Box::new(Session::new()));
    let reply = talk_on(
        &mut free,
        &lines(&[request(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
        )]),
    );
    let words = reply[0]["result"]["instructions"].as_str().unwrap();
    assert!(words.contains("not limited"), "{words}");
}

#[test]
fn with_no_roots_in_force_nothing_changes() {
    let world = World::new();
    let mut backend = Backend::Headless(Box::new(Session::new()));
    let replies =
        talk_on(&mut backend, &lines(&[call(2, "command_run", json!({"id": "file.open", "params": {"path": world.p("outside/drawing.diacraft")}}))]));
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
}

#[cfg(unix)]
#[test]
fn a_link_that_leads_out_of_a_root_is_not_followed_through_the_tools() {
    let world = World::new();
    std::os::unix::fs::symlink(world.base.join("outside/drawing.diacraft"), world.base.join("read/leak.diacraft")).unwrap();
    std::os::unix::fs::symlink(world.base.join("outside"), world.base.join("write/away")).unwrap();
    let mut server = Server::new(&world);
    rule(&world, &server.run("file.open", json!({"path": world.p("read/leak.diacraft")})).unwrap_err(), "--automation-read-root");
    server.run("file.new", json!({})).unwrap();
    rule(&world, &server.run("file.save_as", json!({"path": world.p("write/away/x.diacraft")})).unwrap_err(), "--automation-write-root");
    assert!(!world.exists("outside/x.diacraft"));
}
