//! What the forms of an earlier ODF version that a visit writes in the form 1.4 takes cost (`write/model/forms.rs`, `read/odfdoc.rs`'s `restore_forms`, `read/styles.rs`'s
//! `exact_value`).
//!
//! The walks are of the properties elements of a style kept whole: one step for each attribute, a set for the names it holds and the carriers written, and a map for the namespaces it declares, so
//! that an element of many attributes costs what it has and no more. Each input is crafted as a hostile file is, **beside a yardstick of the same size** whose attributes need no change:
//!
//! * [`an_element_of_many_attributes_each_of_one_attribute_is_charged_and_carried_once`]: 20,000 attributes of `text:enable-numbering` under 20,000 prefixes of the text namespace, on the
//!   paragraph properties of a style nothing names. The save writes one carrier, counts the rest, and charges the steps of the walk by the attributes (a count, in the units the loop walks).
//! * [`an_element_of_many_carriers_is_restored_by_maps_and_counted`]: the file written, with 20,000 carriers under 20,000 prefixes of DiaCraft's namespace and 20,000 weights each carried: the read
//!   restores one attribute and charges each attribute it looks at once.
//! * [`the_chains_of_a_style_with_many_weights_cost_what_they_hold`]: a common style whose text properties carry many weights under many prefixes.
//! * [`the_time_of_the_walks_is_of_the_order_of_the_yardstick`] (`#[ignore]`): the same inputs timed against the yardstick.

use std::time::{Duration, Instant};

use diacraft_odg::{Limits, WriteOptions, WriteWork, read, write_counted};

use crate::drawing::Drawing;
use crate::gate_valid::old;
use crate::visit_styles::{GR1, part, rect, through_codec};

const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const DIA: &str = "urn:app.diacraft:extension";

/// How many attributes the hostile elements hold: far past what an honest file holds, inside what the reader takes.
const N: usize = 20_000;

/// A drawing whose unused automatic paragraph style `P9` holds `n` attributes named `attr` under the prefixes `p0` to `p<n>`, each bound to the namespace `uri` on the element.
fn hostile(n: usize, uri: &str, attr: &str, value: &str) -> Vec<u8> {
    let declarations: String = (0..n).map(|k| format!(r#" xmlns:p{k}="{uri}""#)).collect();
    let attrs: String = (0..n).map(|k| format!(r#" p{k}:{attr}="{value}""#)).collect();
    let style = format!(r#"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties{declarations}{attrs}/></style:style>"#);
    old(&Drawing::a4().auto(GR1).auto(&style).page(&rect("gr1")))
}

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The model writer's save of the file `bytes`, from the model alone, with what it charged.
fn saved(bytes: &[u8]) -> (diacraft_odg::Written, WriteWork) {
    let (doc, _) = read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("not read: {e}"));
    let mut work = WriteWork::default();
    let written = write_counted(&through_codec(&doc), None, &unedited(), &mut work).unwrap_or_else(|e| panic!("not written: {e}"));
    (written, work)
}

/// **An element of 20,000 attributes the schemas have no place for.** Each is a `text:enable-numbering` under a prefix of its own. The save writes one carrier (the first), leaves the other
/// 19,999 out and counts them, and charges the walk by the attributes: the steps it takes beyond the yardstick's (the same element with 20,000 attributes of `fo` that need no change) are
/// at least the attributes the walk looks at and at most a small multiple of them.
#[test]
fn an_element_of_many_attributes_each_of_one_attribute_is_charged_and_carried_once() {
    let (written, work) = saved(&hostile(N, TEXT, "enable-numbering", "false"));
    let (_, yardstick) = saved(&hostile(N, FO, "margin-left", "1cm"));
    let content = part(&written.bytes, "content.xml");
    assert_eq!(content.matches("older-enable-numbering=").count(), 1, "one carrier: the other attributes name the same one");
    assert!(
        !content.contains("enable-numbering=\"false\"\n") && !content.contains(":enable-numbering="),
        "none of the attributes is written as it stands"
    );
    let counted = written.notes.iter().any(|n| matches!(n, diacraft_odg::SaveNote::DocumentDataUnwritten { styles, .. } if *styles >= N - 1));
    assert!(counted, "the attributes that could not be carried are counted: {:?}", written.notes);
    // The walks the writer makes are one step for each attribute of the properties element: both inputs hold N, so the two are the same to within the constant of the rest of the save.
    let (more, less) = (work.steps_used().max(yardstick.steps_used()), work.steps_used().min(yardstick.steps_used()));
    assert!(
        more - less <= 64,
        "the walk of the hostile element charges what the yardstick's does: {} against {}",
        work.steps_used(),
        yardstick.steps_used()
    );
    assert!(work.steps_used() >= N as u64, "an attribute is a step: {}", work.steps_used());
}

/// **An element of 20,000 carriers.** The file written, hostile: the paragraph properties of a style nothing names hold a carrier of `text:enable-numbering` under each of 20,000 prefixes of
/// DiaCraft's namespace. The read restores the attribute once, in the place the first carrier stood, takes the other carriers out, and sees each attribute once (a count, against the yardstick of 20,000
/// attributes of `fo` that carry nothing).
#[test]
fn an_element_of_many_carriers_is_restored_by_maps_and_counted() {
    let carriers = hostile(N, DIA, "older-enable-numbering", "false");
    let plain = hostile(N, FO, "margin-left", "1cm");
    let report = read(&carriers, &Limits::default()).unwrap().1;
    let yardstick = read(&plain, &Limits::default()).unwrap().1;
    assert!(report.work.attributes <= yardstick.work.attributes + 4 * N as u64, "{} against {}", report.work.attributes, yardstick.work.attributes);
    let (doc, _) = read(&carriers, &Limits::default()).unwrap();
    let kept = doc.ext.get().unwrap().odf.styles.iter().find(|s| &*s.name == "P9").cloned().expect("the style");
    let props = kept
        .element
        .children
        .iter()
        .find_map(|c| if let diacraft_doc::RawNode::Element(e) = c { Some(e.clone()) } else { None })
        .expect("the properties");
    let restored: Vec<(&str, &str)> = props.attrs.iter().filter(|(k, _)| k.ends_with(":enable-numbering")).collect();
    assert_eq!(restored, [("text:enable-numbering", "false")], "one attribute, where the first carrier stood: {:?}", props.attrs.len());
    assert!(props.attrs.iter().all(|(k, _)| !k.contains("older-")), "no carrier is left");
}

/// **A common style whose text properties spell many weights**, each under a prefix of its own (`fo` bound to the standard's namespace by 20,000 prefixes of the root), each 250. Only the first of a
/// name is the weight of the style (the group holds the first of each), and the others are properties no cell reads: the save writes one weight, in the form 1.4 lists, and one carrier, whatever
/// the count, and charges no more than the same style whose weights are 700 (which 1.4 takes as they are).
#[test]
fn the_chains_of_a_style_with_many_weights_cost_what_they_hold() {
    let style = |weight: &str| {
        let weights: String = (0..N).map(|k| format!(r#" p{k}:font-weight="{weight}""#)).collect();
        format!(r#"<style:style style:name="Light" style:family="graphic"><style:text-properties{weights}/></style:style>"#)
    };
    let drawing = |weight: &str| {
        let mut d = Drawing::a4().common(&style(weight)).page(&rect(""));
        d.ns.push_str(&(0..N).map(|k| format!(r#" xmlns:p{k}="{FO}""#)).collect::<String>());
        old(&d)
    };
    let (written, work) = saved(&drawing("250"));
    let (_, yardstick) = saved(&drawing("700"));
    let styles = part(&written.bytes, "styles.xml");
    assert_eq!(styles.matches("older-font-weight=").count(), 1, "one carrier");
    assert!(!styles.contains(":font-weight=\"250\""), "the weight is written in the form 1.4 lists");
    let (more, less) = (work.steps_used().max(yardstick.steps_used()), work.steps_used().min(yardstick.steps_used()));
    assert!(more - less <= 64, "the walk charges what the yardstick's does: {} against {}", work.steps_used(), yardstick.steps_used());
}

/// **Timing, against the yardstick** (`#[ignore]`: no default-run test asserts elapsed time): the hostile element of 20,000 attributes is saved and read in a time of the order of the same
/// element whose attributes need no change. A walk that looked each attribute up in a list of the others would be a quadratic build, and this test would fail.
#[test]
#[ignore = "a timing test: run it by name against the yardstick"]
fn the_time_of_the_walks_is_of_the_order_of_the_yardstick() {
    let (hostile_bytes, plain_bytes) = (hostile(N, TEXT, "enable-numbering", "false"), hostile(N, FO, "margin-left", "1cm"));
    let best_of_three = |job: &dyn Fn()| {
        (0..3)
            .map(|_| {
                let started = Instant::now();
                job();
                started.elapsed()
            })
            .min()
            .unwrap_or_default()
    };
    let yardstick = best_of_three(&|| {
        saved(&plain_bytes);
    });
    let took = best_of_three(&|| {
        saved(&hostile_bytes);
    });
    let allowed = yardstick * 6 + Duration::from_millis(1_500);
    assert!(took <= allowed, "the save of the hostile element took {took:?} against the yardstick's {yardstick:?}");
    let carriers = hostile(N, DIA, "older-enable-numbering", "false");
    let yardstick = best_of_three(&|| {
        read(&plain_bytes, &Limits::default()).unwrap();
    });
    let took = best_of_three(&|| {
        read(&carriers, &Limits::default()).unwrap();
    });
    assert!(
        took <= yardstick * 6 + Duration::from_millis(1_500),
        "the read of the hostile carriers took {took:?} against the yardstick's {yardstick:?}"
    );
}
