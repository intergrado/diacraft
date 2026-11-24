//! `cargo fuzz run vsdx_carry`: a valid package whose DiaCraft solution part, carried items, blob relationships and blob parts come from the fuzzer, read with the carrier.
//!
//! The bytes are a plan (`diacraft_vsdx_fuzz::plan`, read with `arbitrary`'s `Unstructured`): the type of the file (one of the six), parts planted in the package (under `/diacraft/` in several
//! spellings, and outside it with bytes that begin with `OUT`), edits of the solution part's relationships and of its items (an attribute set to a hard value, an item or an element taken out or
//! doubled), the records made true so that the judge goes past what a record says it is, bytes spliced into the part, and the judge's budget (`Limits::max_carry_steps`: none, a few, the default).
//! A package is made of the plan on a seed that carries the data of the other format in every place it rides. A read gives a document or a refusal, and the properties are:
//!
//! * **no panic**, and **a refusal is in words**: a read that fails says why with no control character, and no warning names an entry of the other format (every hard entry name of the plan holds
//!   `ZQX`, and no message does);
//! * **nothing outside `/diacraft/` is taken as a blob**: the bytes of every part of the other format and of every definition of the document are the bytes of a part under `/diacraft/`, and
//!   none begins with `OUT`;
//! * **no macro part reaches a type that allows none** (a stencil, a template or a drawing of a type without macros), by the role or by the name in any case;
//! * the names the model gets are names an `.odg` entry may have, once each by case, and none that a package makes itself;
//! * **the judge's work stays under its budget**: items judged at most once each, the parts of DiaCraft's looked at once for each record, and, when the read did not say it left items out for
//!   the work, twice the steps of stamping plus the nodes copied are at most `max_carry_steps`;
//! * the bytes of each held part are the package's exactly, and a read of the same package twice gives the same document.
#![no_main]

use diacraft_vsdx::{Limits, read_partial, work};
use diacraft_vsdx_fuzz::checks;
use diacraft_vsdx_fuzz::plan::{OUTSIDE, Plan, TOKEN};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let plan = Plan::of(data);
    let Some(bytes) = plan.package() else { return };
    let limits = Limits { max_carry_steps: plan.steps, ..Limits::default() };
    work::reset();
    let (result, report) = read_partial(&bytes, &limits);
    checks::warnings_in_words(&report);
    assert!(report.warnings.iter().all(|w| !w.contains(TOKEN)), "a warning names an entry of the other format: {:?}", report.warnings);
    checks::judge_stays_in_budget(plan.steps, &report);
    match result {
        Ok(doc) => {
            doc.check_limits().expect("a document that read passes the model's own caps");
            checks::held_is_the_packages(&bytes, &doc, &report);
            checks::carried_parts_are_sound(&bytes, &doc, &report);
            checks::nothing_of_outside_is_carried(OUTSIDE, &doc);
            if data.len().is_multiple_of(4) {
                let (again, second) = read_partial(&bytes, &limits);
                assert!(again.as_ref().is_ok_and(|d| *d == doc) && second == report, "reading twice gave two documents");
            }
        }
        Err(e) => {
            checks::refusal_in_words(&e);
            assert!(!e.to_string().contains(TOKEN), "a refusal names an entry of the other format: {e}");
        }
    }
});
