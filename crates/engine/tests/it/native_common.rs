//! What the `.diacraft` tests of the engine share: a scratch folder, a drawing built in code, and the way a session is made to hold one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use serde_json::{Value, json};

use crate::{blank_session, run};

/// The clock the tests pin (a day in 2030): a model writer's modified time comes from the session's clock.
pub const PINNED: i64 = 1_900_000_000;

/// A scratch folder under `TMPDIR`, removed on drop.
pub struct Dir(PathBuf);

impl Dir {
    pub fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-native-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    /// Writes `bytes` as `name` and gives its path.
    pub fn write(&self, name: &str, bytes: &[u8]) -> String {
        std::fs::write(self.0.join(name), bytes).unwrap();
        self.path(name)
    }

    pub fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.0.join(name)).unwrap()
    }

    pub fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A drawing of two pages with a shape of its own and a part nothing models: what a person's file holds.
pub fn vsdx_drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#)))
        .page(PageSpec::new(2, "Second"))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build()
}

/// A session with the clock pinned and a blank drawing.
pub fn pinned() -> Session {
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    s
}

/// A session with the clock pinned that has opened `path`.
pub fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("{path} did not open: {e}"));
    s
}

/// A drawing a person made: shapes on the first page, a second page and a theme, so that the file holds all that a drawing does.
pub fn made(s: &mut Session) {
    run(s, "theme.apply", json!({ "theme": "fernwood" }));
    for k in 0..3 {
        run(s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 2.0 + f64::from(k), "y": 3.0 }));
    }
    run(s, "page.add", json!({ "name": "Second" }));
}

/// The result of `file.status`.
pub fn status(s: &mut Session) -> Value {
    run(s, "file.status", json!({}))
}
