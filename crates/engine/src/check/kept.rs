//! What a format keeps beside the cells (attributes and elements nothing reads, held values, the VSDX elements a slot had no place for): whether it all came back.
//!
//! **The direction is one way.** The check asks whether what the drawing held is in the file read back, and never whether the file holds more: a format may add what it needs (an automatic style
//! name, a stamp, an attribute a reader sets), and adding is no loss. Stamps and automatic style names are the writers' own bookkeeping and are not compared.
//!
//! **Every scan is charged first** (the [`Meter`]): a list matched against another costs `len * len` steps, taken before the first comparison, and a comparison past its budget answers "kept"
//! (it found no difference) and leaves the walk to stop with [`super::meter::Stop`]. The usual case, the file's list the drawing's in the same order, is linear.

use diacraft_doc::{Held, HeldGroup, Kept, Passthrough, RawAttrs, VsdxRest};

use super::class::Class;
use super::meter::Meter;

/// Whether every attribute of `before` is in `after`, with the same value.
pub(crate) fn attrs_kept(before: &RawAttrs, after: &RawAttrs, meter: &Meter) -> bool {
    if before.is_empty() {
        return true;
    }
    if !meter.charge(before.len().saturating_mul(after.len().max(1))) {
        return true;
    }
    before.iter().all(|(name, value)| after.get(name) == Some(value))
}

/// Whether every node of `before` is in `after`: the same node, in any place (the place is the writer's to choose).
pub(crate) fn nodes_kept(before: &Passthrough, after: &Passthrough, meter: &Meter) -> bool {
    if before.is_empty() {
        return true;
    }
    // The usual case, in linear time: the nodes of the drawing are the file's in the same order, with any between them.
    if !meter.charge(before.len().saturating_add(after.len())) {
        return true;
    }
    let mut rest = after.iter();
    if before.iter().all(|wanted| rest.any(|have| have.node == wanted.node)) {
        return true;
    }
    // Nodes the file put in another order: matched one by one, and paid for first, the cost being the product of the two lengths.
    if !meter.charge(before.len().saturating_mul(after.len())) {
        return true;
    }
    let mut used = vec![false; after.len()];
    before.iter().all(|wanted| {
        let found = after.iter().enumerate().find(|(at, have)| used.get(*at) == Some(&false) && have.node == wanted.node);
        match found {
            Some((at, _)) => {
                if let Some(slot) = used.get_mut(at) {
                    *slot = true;
                }
                true
            }
            None => false,
        }
    })
}

/// Whether what `before` kept is all in `after`.
pub(crate) fn kept_kept(before: &Kept, after: &Kept, meter: &Meter) -> bool {
    attrs_kept(&before.raw, &after.raw, meter) && nodes_kept(&before.kept, &after.kept, meter)
}

/// Whether the VSDX elements an object had no slot for are all in `after`, each as it was.
pub(crate) fn rest_kept(before: &VsdxRest, after: &VsdxRest, meter: &Meter) -> bool {
    if before.0.is_empty() || !meter.charge(before.0.len().saturating_mul(after.0.len().max(1))) {
        return true;
    }
    before.0.iter().all(|(key, kept)| after.0.iter().any(|(other, found)| other == key && kept_kept(kept, found, meter)))
}

/// The class a held value is told in when it does not come back.
pub(crate) fn held_class(held: &Held) -> Class {
    match held.group {
        HeldGroup::Fill => {
            let gradient = held.attrs.iter().any(|(name, value)| name == "draw:fill-gradient-name" || (name == "draw:fill" && value == "gradient"));
            if gradient { Class::Gradients } else { Class::StyleContent }
        }
        HeldGroup::LineEnds => Class::LineEnds,
        HeldGroup::Line | HeldGroup::Shadow => Class::StyleContent,
        HeldGroup::Text => Class::TextFormatting,
        HeldGroup::Frame | HeldGroup::Geometry => Class::Geometry,
        HeldGroup::Glue => Class::Connections,
    }
}

/// The classes of the held values of `before` that are not in `after`: a value is in when `after` holds one of its group with the same attributes and the same kept children (the stamp is
/// the reader's bookkeeping), each of `after`'s answering one of `before`'s only once. As a set of class bits.
pub(crate) fn held_lost(before: &[Held], after: &[Held], meter: &Meter) -> u32 {
    if before.is_empty() || !meter.charge(before.len().saturating_mul(after.len().max(1))) {
        return 0;
    }
    let mut used = vec![false; after.len()];
    let mut lost = 0;
    for wanted in before {
        let found = after
            .iter()
            .enumerate()
            .find(|(at, have)| used.get(*at) == Some(&false) && have.group == wanted.group && have.attrs == wanted.attrs && have.kept == wanted.kept);
        match found {
            Some((at, _)) => {
                if let Some(slot) = used.get_mut(at) {
                    *slot = true;
                }
            }
            None => lost |= held_class(wanted).bit(),
        }
    }
    lost
}

#[cfg(test)]
mod tests {
    use diacraft_doc::RawNode;

    use super::*;

    fn held(group: HeldGroup, attrs: &[(&str, &str)]) -> Held {
        Held { group, stamp: None, attrs: RawAttrs::from_pairs(attrs.iter().copied()), kept: Passthrough::new() }
    }

    fn meter() -> Meter {
        Meter::new(u64::MAX)
    }

    fn nodes(texts: &[&str]) -> Passthrough {
        Passthrough::from_nodes(texts.iter().map(|t| diacraft_doc::Placed { after: 0, node: RawNode::Text(std::sync::Arc::from(*t)) }).collect())
    }

    #[test]
    fn attributes_that_are_in_the_file_with_more_beside_them_are_kept() {
        let before = RawAttrs::from_pairs([("a", "1")]);
        assert!(attrs_kept(&before, &RawAttrs::from_pairs([("b", "2"), ("a", "1")]), &meter()));
        assert!(!attrs_kept(&before, &RawAttrs::from_pairs([("a", "2")]), &meter()));
        assert!(!attrs_kept(&before, &RawAttrs::new(), &meter()));
        assert!(attrs_kept(&RawAttrs::new(), &RawAttrs::new(), &meter()));
    }

    #[test]
    fn nodes_in_the_same_order_are_kept_in_linear_steps_and_in_another_order_are_paid_for_first() {
        let (before, after) = (nodes(&["a", "b", "c"]), nodes(&["a", "x", "b", "c"]));
        let linear = Meter::new(u64::MAX);
        assert!(nodes_kept(&before, &after, &linear));
        assert_eq!(linear.steps(), 7, "one pass over both lists");
        let shuffled = Meter::new(u64::MAX);
        assert!(nodes_kept(&before, &nodes(&["c", "b", "a"]), &shuffled), "in another order they are still kept");
        assert_eq!(shuffled.steps(), 6 + 9, "the pass, and then the product, charged before the matching");
        assert!(!nodes_kept(&before, &nodes(&["c", "b"]), &meter()));
    }

    #[test]
    fn a_comparison_past_its_budget_answers_kept_and_leaves_the_walk_to_stop() {
        let meter = Meter::new(8);
        // The product of the two lists is more than the budget has left: no matching is done, and nothing is said to differ.
        assert!(nodes_kept(&nodes(&["a", "b", "c"]), &nodes(&["c", "b", "x"]), &meter));
        assert!(meter.exhausted());
        let meter = Meter::new(3);
        assert!(attrs_kept(&RawAttrs::from_pairs([("a", "1"), ("b", "2")]), &RawAttrs::from_pairs([("a", "9"), ("b", "2")]), &meter));
        assert!(meter.exhausted());
        assert_eq!(held_lost(&[held(HeldGroup::LineEnds, &[])], &vec![held(HeldGroup::Fill, &[]); 5], &Meter::new(2)), 0);
    }

    #[test]
    fn a_held_value_is_in_the_file_once_for_each_time_it_was_held_and_is_told_by_its_group() {
        let line_end = held(HeldGroup::LineEnds, &[("draw:marker-start", "m1")]);
        let gradient = held(HeldGroup::Fill, &[("draw:fill", "gradient"), ("draw:fill-gradient-name", "g1")]);
        let hatch = held(HeldGroup::Fill, &[("draw:fill", "hatch")]);
        let twin = held(HeldGroup::Geometry, &[]);
        let m = meter();
        assert_eq!(held_lost(&[line_end.clone(), gradient.clone()], &[gradient.clone(), line_end.clone()], &m), 0);
        assert_eq!(
            held_lost(&[line_end.clone(), gradient.clone(), hatch], std::slice::from_ref(&line_end), &m),
            Class::Gradients.bit() | Class::StyleContent.bit()
        );
        assert_eq!(held_lost(&[twin.clone(), twin.clone()], std::slice::from_ref(&twin), &m), Class::Geometry.bit(), "two held, one back");
        assert_eq!(held_lost(&[line_end], &[], &m), Class::LineEnds.bit());
    }
}
