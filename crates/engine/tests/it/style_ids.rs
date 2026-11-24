//! A VSDX drawing whose style sheets are numbered with gaps is saved as an `.odg` (it was refused in words: "a style sheet whose ID is not its place among the style
//! sheets") and comes back, through the `.odg` and through a `.diacraft`, as a VSDX file with the style sheets it had: their `ID`s, names, links and cells.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::run;

/// A rectangle with its own cells and a text, in the style sheet `style` (all three links).
fn vsdx_rect(id: u32, x: f64, style: u32) -> String {
    format!(
        concat!(
            r#"<Shape ID="{id}" Type="Shape" LineStyle="{style}" FillStyle="{style}" TextStyle="{style}"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/>"#,
            r#"<Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
            r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
            r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
            r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section><Text>hello</Text></Shape>"#
        ),
        id = id,
        x = x,
        style = style
    )
}

/// The style sheets of the numbering the task names: IDs 0, 1, 2, 6, 7 and 30, a root with cells of its own, and links that name each other (`Box` follows `Normal` for its line and its text and the
/// root for its fill).
fn styles() -> String {
    concat!(
        r#"<StyleSheet ID="0" NameU="No Style" Name="No Style" IsCustomNameU="1"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/>"#,
        r##"<Cell N="LineWeight" V="0.01"/><Cell N="FillForegnd" V="#ffffff"/></StyleSheet>"##,
        r#"<StyleSheet ID="1" NameU="Normal" Name="Normal" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.02"/></StyleSheet>"#,
        r##"<StyleSheet ID="2" NameU="Heading" Name="Heading" LineStyle="1" FillStyle="1" TextStyle="1"><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##,
        r##"<StyleSheet ID="6" NameU="Box" Name="Box" LineStyle="1" FillStyle="0" TextStyle="1"><Cell N="FillForegnd" V="#c4a000"/></StyleSheet>"##,
        r##"<StyleSheet ID="7" NameU="Accent" Name="Accent" LineStyle="6" FillStyle="6" TextStyle="6"><Cell N="LineWeight" V="0.04"/></StyleSheet>"##,
        r##"<StyleSheet ID="30" NameU="Last" Name="Last" LineStyle="7" FillStyle="7" TextStyle="7"><Cell N="FillForegnd" V="#cc0000"/></StyleSheet>"##
    )
    .to_owned()
}

/// A VSDX drawing with those style sheets and four rectangles, on style sheets 6, 30, 2 and the root (the one a theme is applied to: a shape that has a style sheet of its own follows none).
fn vsdx_gapped() -> Vec<u8> {
    VsdxBuilder::new()
        .styles(&styles())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&format!(
            "{}{}{}{}",
            vsdx_rect(1, 2.0, 6),
            vsdx_rect(2, 5.0, 30),
            vsdx_rect(3, 8.0, 2),
            vsdx_rect(4, 11.0, 0)
        )))
        .build()
}

fn open_file(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, bytes) }));
    s
}

fn open_path(dir: &Dir, name: &str) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Value {
    s.execute("file.save_as", &json!({ "path": dir.path(name) })).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// What identifies the style sheets of a drawing: the `ID`, the names and the three links, and the cells' values.
type Sheets = Vec<(u32, Option<String>, Option<String>, [Option<u32>; 3], Vec<(String, String)>)>;

fn style_sheets(doc: &Document) -> Sheets {
    doc.styles
        .iter()
        .map(|s| {
            let cells = s
                .sheet
                .cells
                .iter()
                .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()))
                .collect();
            (
                s.id,
                s.name.as_deref().map(str::to_owned),
                s.name_u.as_deref().map(str::to_owned),
                [s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text],
                cells,
            )
        })
        .collect()
}

fn read_vsdx(bytes: &[u8]) -> (Document, diacraft_vsdx::ReadReport) {
    diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn read_odg(bytes: &[u8]) -> Document {
    diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{e}")).0
}

/// The `ID`s of a drawing's style sheets, in their order.
fn ids(doc: &Document) -> Vec<u32> {
    doc.styles.iter().map(|s| s.id).collect()
}

/// **The task's trip.** A VSDX drawing whose style sheets are numbered 0, 1, 2, 6, 7 and 30 is saved as an `.odg` (it was refused), read back with those `ID`s, saved as a `.diacraft`, opened, and
/// saved as a VSDX file: the style sheets come back with their `ID`s, names, links and cells, and no item of what the formats carried is lost.
#[test]
fn a_vsdx_drawing_with_gaps_in_its_style_ids_goes_through_an_odg_and_a_diacraft_and_back_to_vsdx() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_gapped());
    let first = s.document().unwrap().clone();
    assert_eq!(ids(&first), [0, 1, 2, 6, 7, 30], "the drawing as the VSDX file numbers it");
    let out = save_as(&mut s, &dir, "b.odg");
    assert_eq!((out["variant"].as_str(), out["kind"].as_str()), (Some("odg"), Some("drawing")));
    // The check after the save reads the file back: the style sheets are no class that differs (what it says is lost is the formulas an `.odg` holds as values).
    let classes: Vec<&str> = out["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
    assert_eq!(out["checked"]["done"], true, "{}", out["checked"]);
    assert!(!classes.contains(&"style-sheets") && !classes.contains(&"style-content"), "{classes:?}");
    // The `.odg` holds the numbering as it was.
    let odg = read_odg(&dir.read("b.odg"));
    assert_eq!(style_sheets(&odg), style_sheets(&first), "the `.odg` reads with the style sheets the VSDX file had");
    // The `.odg` is opened, saved as a `.diacraft`, which is opened and saved as a VSDX file.
    let mut t = open_path(&dir, "b.odg");
    assert_eq!(ids(t.document().unwrap()), [0, 1, 2, 6, 7, 30], "the drawing opened from the `.odg`");
    save_as(&mut t, &dir, "c.diacraft");
    let mut u = open_path(&dir, "c.diacraft");
    assert_eq!(style_sheets(u.document().unwrap()), style_sheets(&first), "the `.diacraft` holds them");
    save_as(&mut u, &dir, "d.vsdx");
    let (back, report) = read_vsdx(&dir.read("d.vsdx"));
    assert_eq!(
        (report.carry.stale, report.carry.gone, report.carry.left_out),
        (0, 0, 0),
        "nothing of what the formats carried is lost: {:?}",
        report.carry
    );
    assert_eq!(style_sheets(&back), style_sheets(&first), "the style sheets: IDs, names, links and cells");
    // The drawing from the `.odg` is saved into its file as it is, and after an edit, with no refusal for its numbering.
    run(&mut t, "file.save", json!({}));
    run(&mut t, "page.size", json!({ "width": 8.0, "height": 10.0 }));
    run(&mut t, "file.save", json!({}));
    let again = read_odg(&dir.read("b.odg"));
    assert_eq!(ids(&again), [0, 1, 2, 6, 7, 30], "a save into the file keeps the numbering");
}

/// **A style sheet the engine adds takes the number after the greatest** (31, a place of 6): the `Theme` style a theme command makes, and a shape that follows it, are saved as an `.odg` and read back with the
/// `ID` the style has, and the shape on it.
#[test]
fn the_theme_style_added_after_gaps_is_numbered_after_the_greatest_and_a_shape_that_follows_it_comes_back_on_it() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_gapped());
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "variant": 1, "scope": "all"}));
    let model = s.document().unwrap().clone();
    let theme = model.styles.iter().find(|st| st.name_u.as_deref() == Some("Theme")).map(|st| st.id).expect("a theme style");
    assert_eq!(theme, 31, "the number after the greatest, and not its place (6)");
    let followers: Vec<u32> =
        model.pages.iter().flat_map(|p| p.shapes.iter()).filter(|sh| sh.sheet.links.line == Some(theme)).map(|sh| sh.id).collect();
    assert!(!followers.is_empty(), "shapes follow the theme: the three links of a shape the theme was applied to name it");
    save_as(&mut s, &dir, "b.odg");
    let odg = read_odg(&dir.read("b.odg"));
    assert_eq!(ids(&odg), ids(&model));
    assert_eq!(style_sheets(&odg), style_sheets(&model));
    let on: Vec<u32> = odg.pages.iter().flat_map(|p| p.shapes.iter()).filter(|sh| sh.sheet.links.line == Some(theme)).map(|sh| sh.id).collect();
    assert_eq!(on, followers, "the shapes that followed the theme follow it in the file");
    // And a VSDX file made from the `.odg` names it by its number.
    let mut t = open_path(&dir, "b.odg");
    save_as(&mut t, &dir, "c.vsdx");
    let (back, _) = read_vsdx(&dir.read("c.vsdx"));
    assert_eq!(style_sheets(&back), style_sheets(&model));
}

/// **A theme applied to a drawing opened from the `.odg` that was written** (the splice writer, onto a file whose numbering has gaps): the `Theme` style takes the number after the greatest (31, at the place
/// 6) and is written beside the file's own style sheets with the number it has, the shapes that follow it are saved on it, and the file reads back as the drawing is.
#[test]
fn a_theme_applied_to_a_drawing_opened_from_a_gapped_odg_is_saved_into_the_file_with_its_number() {
    let dir = Dir::new();
    let mut s = open_file(&dir, "a.vsdx", &vsdx_gapped());
    save_as(&mut s, &dir, "b.odg");
    let mut t = open_path(&dir, "b.odg");
    run(&mut t, "theme.apply", json!({"theme": "tidepool", "variant": 1, "scope": "all"}));
    let model = t.document().unwrap().clone();
    let theme = model.styles.iter().find(|st| st.name_u.as_deref() == Some("Theme")).map(|st| st.id).expect("a theme style");
    assert_eq!(theme, 31, "the number after the greatest, which is not the place the style is at");
    let followers: Vec<u32> =
        model.pages.iter().flat_map(|p| p.shapes.iter()).filter(|sh| sh.sheet.links.line == Some(theme)).map(|sh| sh.id).collect();
    assert!(!followers.is_empty());
    run(&mut t, "file.save", json!({}));
    let back = read_odg(&dir.read("b.odg"));
    assert_eq!(ids(&back), [0, 1, 2, 6, 7, 30, 31], "the numbering as it was, and the theme's after it");
    assert_eq!(style_sheets(&back), style_sheets(&model));
    let on: Vec<u32> = back.pages.iter().flat_map(|p| p.shapes.iter()).filter(|sh| sh.sheet.links.line == Some(theme)).map(|sh| sh.id).collect();
    assert_eq!(on, followers, "the shapes that follow the theme are saved on it");
}

/// Writes the files a LibreOffice run is given into the folder `STYLEIDS_LO_DIR` names (by hand: `STYLEIDS_LO_DIR=<folder> cargo test -p diacraft-engine --test it -- --ignored style_ids::lo_files`): the
/// VSDX drawing with the gapped style sheets, saved across as an `.odg` and an `.otg`, with and without a theme, and the style sheets with no gap saved across (which must be the same bytes before and
/// after this task). Returns at once with no folder named.
#[test]
#[ignore = "writes files for a LibreOffice run, by hand"]
fn lo_files() {
    let Some(folder) = std::env::var_os("STYLEIDS_LO_DIR") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let path = |name: &str| folder.join(name).to_str().unwrap().to_owned();
    let try_save = |from: &str, to: &str, theme: bool| {
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": path(from) }));
        if theme {
            run(&mut s, "theme.apply", json!({"theme": "tidepool", "variant": 1, "scope": "all"}));
        }
        match s.execute("file.save_as", &json!({ "path": path(to) })) {
            Ok(_) => {}
            Err(e) => println!("NOT WRITTEN {to}: {e}"),
        }
    };
    std::fs::write(folder.join("gap.vsdx"), vsdx_gapped()).unwrap();
    try_save("gap.vsdx", "gap-across.odg", false);
    try_save("gap.vsdx", "gap-across.otg", false);
    try_save("gap.vsdx", "gap-theme.odg", true);
    // The same drawing numbered by its places: a VSDX file with the style sheets 0, 1 and 2.
    let plain = VsdxBuilder::new()
        .styles(concat!(
            r##"<StyleSheet ID="0" NameU="No Style" Name="No Style" IsCustomNameU="1"><Cell N="LineWeight" V="0.01"/><Cell N="FillForegnd" V="#ffffff"/></StyleSheet>"##,
            r#"<StyleSheet ID="1" NameU="Normal" Name="Normal" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.02"/></StyleSheet>"#,
            r##"<StyleSheet ID="2" NameU="Box" Name="Box" LineStyle="1" FillStyle="0" TextStyle="1"><Cell N="FillForegnd" V="#3465a4"/></StyleSheet>"##
        ))
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&format!("{}{}", vsdx_rect(1, 2.0, 2), vsdx_rect(2, 5.0, 1))))
        .build();
    std::fs::write(folder.join("nogap.vsdx"), plain).unwrap();
    try_save("nogap.vsdx", "nogap-across.odg", false);
    try_save("nogap.vsdx", "nogap-across.otg", false);
}
