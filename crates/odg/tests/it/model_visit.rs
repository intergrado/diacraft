//! An `.odg` visit through DiaCraft: a drawing read from an `.odg`, written from the model alone, has the same content as the file (`compare_content`).
use diacraft_odg::{Limits, WriteOptions, compare_content, read, write_with};

use crate::build::package;

/// The parts a visit is judged by: what the drawing says. The package's own bookkeeping (the manifest, `meta.xml`, DiaCraft's document file) is the document level's, which the tests judge.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// The visit of one case: the number of differences from the file in the parts of the drawing, and the first of them.
fn visit(bytes: &[u8]) -> Result<(usize, Vec<String>), String> {
    let (doc, _) = read(bytes, &Limits::default()).map_err(|e| format!("does not read: {e}"))?;
    let written =
        write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).map_err(|e| format!("refused: {e}"))?;
    let diff = compare_content(bytes, &written.bytes, &Limits::default()).map_err(|e| format!("compare: {e}"))?;
    let ours: Vec<String> =
        diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    Ok((ours.len(), ours))
}

#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure() {
    let (mut same, mut total) = (0, 0);
    for (name, drawing) in crate::model_measure::cases() {
        let mut drawing = drawing;
        drawing.version = "1.4".to_owned();
        let bytes = package(crate::build::DRAWING, drawing.files());
        total += 1;
        match visit(&bytes) {
            Ok((0, _)) => {
                same += 1;
                println!("VISIT {name:<40} same");
            }
            Ok((n, shown)) => {
                println!("VISIT {name:<40} {n} differences");
                for line in shown.iter().take(8) {
                    println!("        {}", line.chars().take(220).collect::<String>());
                }
            }
            Err(words) => println!("VISIT {name:<40} {}", words.chars().take(200).collect::<String>()),
        }
    }
    println!("VISITS {same} of {total} the same");
}

/// Prints the parts of the case named by `DUMP_CASE` as the model writer writes them (an exploration aid).
#[test]
#[ignore = "an exploration: run by hand with --nocapture and DUMP_CASE"]
fn dump() {
    let want = std::env::var("DUMP_CASE").unwrap_or_default();
    for (name, drawing) in crate::model_measure::cases() {
        if name != want {
            continue;
        }
        let bytes = package(crate::build::DRAWING, drawing.files());
        let (doc, _) = read(&bytes, &Limits::default()).unwrap();
        let written = write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
        let pkg = diacraft_odg::package::Package::read(&written.bytes, &Limits::default()).unwrap();
        for part in ["content.xml", "styles.xml", "DiaCraft/document.xml"] {
            println!("==== {part}\n{}", pkg.part(part).map_or("<none>".to_owned(), |b| String::from_utf8_lossy(b).into_owned()));
        }
        println!("==== ORIGINAL content.xml\n{}", drawing.content_xml());
        println!("==== NOTES {:?}", written.notes);
    }
}

/// What a page of the case named by `DUMP_CASE` holds that a new page may not (an exploration aid).
#[test]
#[ignore = "an exploration: run by hand with --nocapture and DUMP_CASE"]
fn page_fields() {
    let want = std::env::var("DUMP_CASE").unwrap_or_default();
    for (name, drawing) in crate::model_measure::cases() {
        if name != want {
            continue;
        }
        let (doc, _) = read(&package(crate::build::DRAWING, drawing.files()), &Limits::default()).unwrap();
        for (i, page) in doc.pages.iter().enumerate() {
            println!(
                "PAGE {i} background={} back_page={:?} rel_id={:?} raw={} passthrough={} odf={:?}",
                page.background,
                page.back_page,
                page.rel_id,
                page.raw.len(),
                page.passthrough.len(),
                page.ext.get().map(|d| (d.odf.kept.raw.len(), d.odf.kept.kept.len(), d.odf.master_page.clone(), d.odf.layout.clone()))
            );
        }
    }
}

/// What the first shape of the case named by `DUMP_CASE` holds, as the reader fills it (an exploration aid).
#[test]
#[ignore = "an exploration: run by hand with --nocapture and DUMP_CASE"]
fn shape_fields() {
    let want = std::env::var("DUMP_CASE").unwrap_or_default();
    for (name, drawing) in crate::model_measure::cases() {
        if name != want {
            continue;
        }
        let (doc, _) = read(&package(crate::build::DRAWING, drawing.files()), &Limits::default()).unwrap();
        let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
        println!("KIND {:?} id={} name={:?} master={:?}", shape.kind, shape.id, shape.name_u, shape.master);
        println!("LINKS {:?}", shape.sheet.links);
        for (n, c) in shape.sheet.cells.iter() {
            println!(
                "CELL {} formula={:?} value={:?} unit={:?}",
                n.as_str(),
                c.formula.text(),
                c.value.as_ref().map(|v| v.text().to_owned()),
                c.unit
            );
        }
        for s in shape.sheet.sections.iter() {
            println!("SECTION {} ix={:?} rows={}", s.name.as_str(), s.ix, s.rows.len());
        }
        let ext = shape.ext.get();
        println!("EXT {:#?}", ext.as_ref().map(|d| &d.odf));
        let dext = doc.ext.get();
        println!("DOCODF styles={:#?}", dext.as_ref().map(|d| (&d.odf.styles, d.odf.definitions.len(), d.odf.layers.len())));
    }
}

/// A black-box check against LibreOffice, run by hand (ignored): writes the drawings of the 24 vectors, as the model writer writes them after a visit through the model, and a few of
/// this task's own (the OLE object, held values, two pages) into `$ODG_OUT_DIR/in`, plus a control, a package whose manifest and parts say two versions. Then
/// `soffice -env:UserInstallation=file://$ODG_OUT_DIR/profile --headless --convert-to pdf --outdir $ODG_OUT_DIR/out $ODG_OUT_DIR/in/<file>` for each of them: only a missing output file is a
/// failure (LibreOffice offers a repair for a package it finds damaged, and headless it then loads nothing).
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_OUT_DIR set to an empty folder"]
fn lo_model_shapes() {
    let dir = std::path::PathBuf::from(std::env::var("ODG_OUT_DIR").expect("set ODG_OUT_DIR to an empty folder")).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, (name, drawing)) in crate::model_measure::cases().into_iter().enumerate() {
        let mut drawing = drawing;
        drawing.version = "1.3".to_owned();
        let (doc, _) = read(&package(crate::build::DRAWING, drawing.files()), &Limits::default()).unwrap();
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        std::fs::write(dir.join(format!("vector{n:02}.odg")), &written.bytes).unwrap();
    }
    // The control: the same package with the manifest's root row saying version 1.3 over parts that say 1.4, which LibreOffice 26.8.2.2 refuses to load (measured: the
    // check has teeth).
    let (doc, _) = read(&package(crate::build::DRAWING, crate::model_measure::cases().remove(0).1.files()), &Limits::default()).unwrap();
    let good = write_with(&doc, None, &WriteOptions::default()).unwrap().bytes;
    let package = diacraft_odg::package::Package::read(&good, &Limits::default()).unwrap();
    let mut writer = diacraft_container::zip::ArchiveWriter::new();
    writer.add("mimetype", diacraft_odg::Variant::Drawing.media_type().as_bytes(), diacraft_container::zip::Method::Stored).unwrap();
    for name in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        writer.add(name, package.part(name).unwrap(), diacraft_container::zip::Method::Deflated).unwrap();
    }
    let manifest = String::from_utf8(package.part("META-INF/manifest.xml").unwrap().to_vec()).unwrap();
    let wrong = manifest.replacen("manifest:full-path=\"/\" manifest:version=\"1.4\"", "manifest:full-path=\"/\" manifest:version=\"1.3\"", 1);
    assert_ne!(wrong, manifest, "the root row said 1.4");
    writer.add("META-INF/manifest.xml", wrong.as_bytes(), diacraft_container::zip::Method::Deflated).unwrap();
    std::fs::write(dir.join("control_version_mismatch.odg"), writer.finish().unwrap()).unwrap();
}

/// What a second model save of a drawing the first one wrote is, per vector: the number of differences of content between the two files.
#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure_second_save() {
    for (name, drawing) in crate::model_measure::cases() {
        let mut drawing = drawing;
        drawing.version = "1.4".to_owned();
        let (doc, _) = read(&package(crate::build::DRAWING, drawing.files()), &Limits::default()).unwrap();
        let options = WriteOptions { content_unchanged: true, ..WriteOptions::default() };
        let first = write_with(&doc, None, &options).unwrap();
        let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
        let second = match write_with(&again, None, &options) {
            Ok(w) => w,
            Err(e) => {
                println!("SECOND {name:<40} refused: {e}");
                continue;
            }
        };
        let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
        println!("SECOND {name:<40} {} differences", diff.count);
        for d in diff.shown.iter().take(4) {
            println!("        {} {} {:?}", d.part, d.path, d.found);
        }
    }
}

/// Prints the written content of the vector `names_and_units` (an exploration aid).
#[test]
#[ignore = "an exploration: run by hand with --nocapture"]
fn dump_units() {
    let mut drawing = crate::model_asread::names_and_units_for_dump();
    drawing.version = "1.4".to_owned();
    let bytes = package(crate::build::DRAWING, drawing.files());
    let (doc, _) = read(&bytes, &Limits::default()).unwrap();
    let written = write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let pkg = diacraft_odg::package::Package::read(&written.bytes, &Limits::default()).unwrap();
    println!("{}", String::from_utf8_lossy(pkg.part("content.xml").unwrap()));
    println!("NOTES {:?}", written.notes);
}

/// Prints the entries whose bytes differ between the first and the second model save of the case named by `DUMP_CASE` (an exploration aid).
#[test]
#[ignore = "an exploration: run by hand with --nocapture and DUMP_CASE"]
fn second_save_entries() {
    let want = std::env::var("DUMP_CASE").unwrap_or_default();
    for (name, drawing) in crate::model_measure::cases() {
        if name != want {
            continue;
        }
        let mut drawing = drawing;
        drawing.version = "1.4".to_owned();
        let (doc, _) = read(&package(crate::build::DRAWING, drawing.files()), &Limits::default()).unwrap();
        let options = WriteOptions { content_unchanged: true, ..WriteOptions::default() };
        let first = write_with(&doc, None, &options).unwrap();
        let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
        let second = write_with(&again, None, &options).unwrap();
        let (a, b) = (
            diacraft_odg::package::Package::read(&first.bytes, &Limits::default()).unwrap(),
            diacraft_odg::package::Package::read(&second.bytes, &Limits::default()).unwrap(),
        );
        let names: Vec<String> = a.parts().chain(b.parts()).map(str::to_owned).collect();
        for n in names {
            if a.part(&n) != b.part(&n) {
                println!("DIFFERS {n}");
                println!("  A {}", String::from_utf8_lossy(a.part(&n).unwrap_or_default()).chars().take(1500).collect::<String>());
                println!("  B {}", String::from_utf8_lossy(b.part(&n).unwrap_or_default()).chars().take(1500).collect::<String>());
            }
        }
        println!("same entries bytes: {}", first.bytes == second.bytes);
    }
}
