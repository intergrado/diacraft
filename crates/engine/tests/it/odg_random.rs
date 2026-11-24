//! Random walks over the engine's `.odg` path: the commands a person uses, in orders no test thought of, each followed by a save. A save is
//! made or refused in the writer's words, never an internal error or a panic; a save that is made reads back as the drawing the session holds; and the same
//! drawing saved again is no edit. The two faults the first reading of this found by hand (the cells of a label a route writes, and the mark of how a page was
//! sized) are the kind it is for.

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::check_session;
use crate::odg_build::{Dir, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four};

/// A small deterministic generator, so that a failure is a seed and a step number.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
    }

    fn unit(&mut self) -> f64 {
        (self.next() % 1000) as f64 / 1000.0
    }
}

/// The `ID`s of the shapes on page 0, and whether each is a connector.
fn shapes(s: &Session) -> Vec<(u32, bool)> {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|x| (x.id, x.sheet.cell("BeginX").is_some())).collect()
}

/// One random command; what it asks may be refused by the engine (a bad parameter), which is no matter.
fn step(s: &mut Session, rng: &mut Rng) -> String {
    let all = shapes(s);
    let boxes: Vec<u32> = all.iter().filter(|(_, c)| !c).map(|(id, _)| *id).collect();
    let pick = |rng: &mut Rng, ids: &[u32]| ids.get(rng.below(ids.len())).copied().unwrap_or(1);
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let masters = ["rectangle", "ellipse", "diamond", "hexagon", "square", "circle", "octagon"];
    let (command, params): (&str, Value) = match rng.below(14) {
        0 | 1 => ("shape.move", json!({ "ids": [pick(rng, &boxes)], "dx": rng.unit() * 2.0 - 1.0, "dy": rng.unit() * 2.0 - 1.0 })),
        2 => {
            let id = pick(rng, &boxes);
            let f = diacraft_engine::draw::shape_frame(s.document().unwrap(), page, id);
            let (x, y, w, h) = f.map_or((1.0, 1.0, 1.0, 1.0), |f| (f.x, f.y, f.width, f.height));
            (
                "shape.resize",
                json!({ "id": id, "x": x, "y": y, "width": (w * (0.5 + rng.unit())).max(0.1), "height": (h * (0.5 + rng.unit())).max(0.1) }),
            )
        }
        3 => ("shape.rotate", json!({ "ids": [pick(rng, &boxes)], "angle": (rng.below(8) * 45) as f64 })),
        4 => ("shape.format", json!({ "ids": [pick(rng, &boxes)], "fill": format!("#{:06x}", rng.next() % 0x0100_0000) })),
        5 => ("shape.format", json!({ "ids": [pick(rng, &boxes)], "weight": 0.5 + rng.unit() * 4.0 })),
        6 => ("shape.set_text", json!({ "id": pick(rng, &boxes), "text": "xy", "start": 0, "end": 0 })),
        7 => {
            let everything: Vec<u32> = all.iter().map(|(id, _)| *id).collect();
            ("shape.delete", json!({ "ids": [pick(rng, &everything)] }))
        }
        8 => {
            let to = ["front", "back", "forward", "backward"][rng.below(4)];
            ("shape.order", json!({ "ids": [pick(rng, &boxes)], "to": to }))
        }
        9 => {
            let master = masters[rng.below(masters.len())];
            ("shape.drop", json!({ "master": master, "page": page, "x": 1.0 + rng.unit() * 6.0, "y": 1.0 + rng.unit() * 8.0 }))
        }
        10 => ("connector.add", json!({ "from": { "id": pick(rng, &boxes) }, "to": { "id": pick(rng, &boxes) } })),
        11 => ("shape.duplicate", json!({ "ids": [pick(rng, &boxes)] })),
        12 => ("edit.undo", json!({})),
        _ => ("text.box", json!({ "page": page, "x": 1.0 + rng.unit() * 6.0, "y": 1.0 + rng.unit() * 8.0, "text": "label" })),
    };
    let _ = s.execute(command, &params);
    format!("{command} {params}")
}

/// Saves; a refusal is the writer's words, after which the person takes steps back until the drawing saves (a session that cannot be saved stays what it is).
fn save_or_take_back(s: &mut Session, log: &mut Vec<String>) -> Option<Value> {
    for _ in 0..40 {
        match s.execute("file.save", &json!({})) {
            Ok(done) => {
                log.push(format!("SAVE edited={}", done["edited"]));
                return Some(done);
            }
            Err(EngineError::Failed { msg, .. }) if msg.contains("the drawing could not be saved") && !msg.contains("internal") => {
                log.push(format!("REFUSED {} -> undo", msg.chars().take(110).collect::<String>()));
                if s.execute("edit.undo", &json!({})).is_err() {
                    return None;
                }
            }
            Err(other) => panic!("a save ended as {other:?}, which is neither a save nor the writer's words"),
        }
    }
    None
}

#[test]
fn random_walks_save_or_are_refused_in_words_and_what_is_saved_reads_back_and_saves_again_as_no_edit() {
    let mut saved = 0;
    let mut refused = 0;
    let seeds: u64 = std::env::var("ODG_SEEDS").ok().and_then(|n| n.parse().ok()).unwrap_or(6);
    for seed in 1..=seeds {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let mut rng = Rng(seed * 7_919);
        let mut done: Vec<String> = Vec::new();
        for round in 0..30 {
            for _ in 0..1 + rng.below(3) {
                done.push(step(&mut s, &mut rng));
            }
            let before = s.history().map_or(0, |h| h.undo_len());
            let Some(result) = save_or_take_back(&mut s, &mut done) else {
                // The drawing could not be saved at all: the person undid everything and it still could not be, which only a drawing the writer cannot hold does.
                continue;
            };
            let after = s.history().map_or(0, |h| h.undo_len());
            refused += usize::from(after < before);
            if result["edited"] == true {
                saved += 1;
                let context = format!("seed {seed}, round {round}: {}", done.join(" ; "));
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
                    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
                }));
                if let Err(payload) = caught {
                    let said = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|t| (*t).to_owned()))
                        .unwrap_or_default();
                    let page = s.document().unwrap().pages.get(0).unwrap().clone();
                    let shapes: Vec<String> = page.shapes.iter().map(|x| format!("{}:{:?}:{}", x.id, x.name_u, x.master.is_some())).collect();
                    let connects: Vec<String> =
                        page.connects.iter().map(|c| format!("{}.{:?}->{}.{:?}", c.from_sheet, c.from_cell, c.to_sheet, c.to_part)).collect();
                    let content = crate::odg_build::content_of(&dir.read("a.odg"));
                    let pieces: Vec<&str> =
                        content.split("<draw:connector ").skip(1).map(|c| c.split("</draw:connector>").next().unwrap_or("")).collect();
                    let ids: Vec<&str> = content.split("xml:id=\"").skip(1).map(|c| c.split('"').next().unwrap_or("")).collect();
                    panic!("{context}\n{said}\nshapes {shapes:?}\nconnects {connects:?}\nCONNECTORS {pieces:#?}\nIDS {ids:?}");
                }
            }
            check_session(&s).unwrap();
        }
    }
    eprintln!("RANDOM saved={saved} refused_and_taken_back={refused}");
    assert!(saved >= 20, "the walks saved {saved} times: they are not walking");
}
