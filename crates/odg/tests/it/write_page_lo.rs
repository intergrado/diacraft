//! Files for the black-box check of a reader the project does not own: each case saves a drawing whose pages were added, renamed, taken away or put in another order,
//! and writes the package into the folder named by `ODG12P_LO_DIR`, where a script converts each with LibreOffice and looks for the output. The test is ignored in a plain run
//! (it writes files and checks nothing a program can check but that the save was made): run it with `--run-ignored only` and the variable set.

use std::sync::Arc;

use diacraft_doc::Document;

use crate::write_core::{Open, open, real};
use crate::write_new::rectangle;
use crate::write_page_ops::{new_page, page_mut, pages_of};

fn write(name: &str, o: &Open) {
    let Ok(dir) = std::env::var("ODG12P_LO_DIR") else { return };
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{name}: {} ({e})", e.class()));
    std::fs::write(std::path::Path::new(&dir).join(name), &saved.bytes).unwrap();
}

fn reverse(doc: &mut Document) {
    let pages: Vec<Arc<diacraft_doc::Page>> = doc.pages.iter().rev().cloned().collect();
    doc.pages = diacraft_doc::SharedVec::from_vec(pages);
}

#[test]
#[ignore = "writes files for a black-box check: set ODG12P_LO_DIR and run with --run-ignored only"]
fn the_files_for_the_black_box_check_of_pages() {
    let mut o = open(&pages_of(3));
    page_mut(&mut o.doc, 1).name = Arc::from("Renamed \"two\" & <more>");
    write("renamed.odg", &o);

    let mut o = open(&pages_of(3));
    o.doc.pages.remove(1);
    write("deleted.odg", &o);

    let mut o = open(&pages_of(4));
    reverse(&mut o.doc);
    write("reversed.odg", &o);

    let mut o = open(&pages_of(2));
    let page = new_page(&o.doc, 2, "Added", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0)), rectangle(2, "Other", (9.0, 9.0, 3.0, 3.0))]);
    o.doc.pages.push(Arc::new(page));
    write("added.odg", &o);

    // A page of another size, which names a copy of the master page, and a page of the first's size that names the first's.
    let mut o = open(&pages_of(2));
    let mut wide = new_page(&o.doc, 2, "Wide", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    {
        let sheet = Arc::make_mut(&mut wide.sheet);
        sheet.cells.get_mut("PageWidth").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(29.7));
        sheet.cells.get_mut("PageHeight").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(42.0));
    }
    o.doc.pages.push(Arc::new(wide));
    let same = new_page(&o.doc, 3, "Same size", Vec::new());
    o.doc.pages.push(Arc::new(same));
    write("added_size.odg", &o);

    // Everything in one save: the first page taken away, the last put first, one renamed, one added.
    let mut o = open(&pages_of(4));
    o.doc.pages.remove(0);
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    page_mut(&mut o.doc, 1).name = Arc::from("Second now");
    let page = new_page(&o.doc, 4, "Added", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    write("all.odg", &o);
}
