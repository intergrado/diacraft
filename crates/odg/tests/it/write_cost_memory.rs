//! The memory a retext holds for the paragraphs it works on: charged to what a save may hold, before it is held.

use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::open;
use crate::write_runs::{chars, set_parts, shape};

fn drawing_of(inner: &str) -> Drawing {
    Drawing::a4().page(&shape("x", "1cm", inner))
}

/// The lists a retext makes of the paragraphs of the two texts and of its plan (about 250 bytes for each paragraph: 479 MB at the cap of 2,000,000 paragraphs) are
/// charged to the memory a save may hold, before they are made (the review, LOW): a text of 100,000 paragraphs that fits the memory the gate estimated
/// beside it by less than the lists need is refused as a save that holds too much memory, once the file was read; with the room, it is written.
#[test]
fn the_paragraph_lists_of_a_retext_are_charged_to_the_memory_a_save_may_hold() {
    use diacraft_odg::{Limits, WriteError};
    let n = 100_000usize;
    let body: String = (0..n).map(|k| format!("<text:p>p{k}</text:p>")).collect();
    let text: String = (0..n).map(|k| if k + 1 == n { "changed".to_owned() } else { format!("p{k}\n") }).collect();
    let mut o = open(&drawing_of(&body));
    set_parts(&mut o, vec![chars(&text)]);
    let estimate = {
        let mut work = WriteWork::default();
        write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
            .unwrap_or_else(|e| panic!("{e}"));
        work.estimated_memory
    };
    let with = |room: u64| {
        let options = WriteOptions { limits: Limits { max_write_memory: estimate + room, ..Limits::default() }, ..WriteOptions::default() };
        let mut work = WriteWork::default();
        let result = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
        (result, work)
    };
    // 200 MB beside the estimate: the lists (about 42 MB) fit.
    let (saved, work) = with(200_000_000);
    assert!(saved.unwrap_or_else(|e| panic!("{e}")).edited);
    let _ = work;
    // 30 MB beside it: the file reads (the read is not what is refused), and the lists do not fit.
    let (refused, work) = with(30_000_000);
    assert!(matches!(refused, Err(WriteError::Limit { .. })), "{:?}", refused.map(|w| w.bytes.len()));
    assert!(work.baseline_read.items > 0, "the file was read, and the text is what was refused");
}
