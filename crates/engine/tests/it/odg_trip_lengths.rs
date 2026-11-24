//! The lengths of a trip. An `.odg` saved as a `.diacraft` drawing and that drawing saved as an `.odg` again (the sample-file visit's trip, `roundtrip --via
//! diacraft`) writes the shapes the second session has no file for from the model: the save flattens them (`odgfile/flatten.rs`), each frame and style cell a constant of the
//! value it resolves to. A constant rounded to nine places of an inch is not the length a metric file gave (2cm is 0.787401574803... inch, and nine places of it read back as
//! 2.0000000005cm), so the members of a group came back as other numbers, a difference of the content check at each of them, where a change planted later went uncounted.

use serde_json::json;

use crate::odg_build::{Dir, Drawing, part_of, rect};
use crate::run;

#[test]
fn a_trip_through_a_diacraft_drawing_writes_a_groups_members_with_the_lengths_the_file_had() {
    let dir = Dir::new();
    let group = format!("<draw:g>{}{}</draw:g>", rect("g1", (1.0, 14.0, 2.0, 2.0), "One"), rect("g2", (4.0, 14.0, 2.0, 2.0), ""));
    let d = Drawing::new().page(&format!("{}{group}", rect("a", (2.0, 3.0, 4.0, 2.0), "Alpha")));
    let source = dir.write("source.odg", &d.bytes());
    let mut first = diacraft_engine::Session::new();
    run(&mut first, "file.open", json!({ "path": source }));
    let native = dir.path("visit.diacraft");
    run(&mut first, "file.save_as", json!({ "path": native }));
    let mut second = diacraft_engine::Session::new();
    run(&mut second, "file.open", json!({ "path": native }));
    let out = dir.path("back.odg");
    run(&mut second, "file.save_as", json!({ "path": out }));
    let content = part_of(&dir.read("back.odg"), "content.xml");
    let group = &content[content.find("<draw:g").expect("a group")..content.find("</draw:g>").expect("its end")];
    for want in [r#"svg:width="2cm""#, r#"svg:height="2cm""#, r#"svg:x="1cm""#, r#"svg:x="4cm""#, r#"svg:y="14cm""#, r#"svg:stroke-width="0.05cm""#] {
        let part = if want.contains("stroke") { content.as_str() } else { group };
        assert!(part.contains(want), "{want}: {group}");
    }
    assert!(!content.contains("0000000"), "no length is a rounded one: {content}");
    // And the content check finds the frames of the group's members the same as the source's (the members' styles and the group's name are other differences).
    let diff = diacraft_odg::compare_content(&dir.read("source.odg"), &dir.read("back.odg"), &diacraft_odg::Limits::default()).unwrap();
    let frames: Vec<String> = diff
        .shown
        .iter()
        .filter(|x| x.path.contains("/g[") && ["/@x", "/@y", "/@width", "/@height"].iter().any(|end| x.path.ends_with(end)))
        .map(|x| format!("{:?} {}", x.found, x.path))
        .collect();
    assert!(frames.is_empty(), "{frames:?}");
}
