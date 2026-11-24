//! What copying a slot costs: **one size function for every slot a put copies** (the scan's fourth round).
//!
//! An item that puts data on an object makes the slot it puts it in its own when others hold it (`Ext::make_mut`, `Arc::make_mut`): the data of the slot is copied. A copy costs the elements
//! of the lists the slot holds, one for each, and the rest of what a slot holds is an `Arc` or a small field, which a copy takes by a reference count. The judge charges that cost to its budget
//! and to the pool before it makes the put (`read.rs`, `copy_charge`), and **every charge site takes the size of a slot from here**.
//!
//! **A type is sized by destructuring all of its fields** (no `..`): a field that a type gains is a compile error here until the size says what it costs, so a list added to a slot cannot be left
//! out of the charge. The model's own figures of what it holds (`diacraft_odg::figures`) are made the same way.

use diacraft_doc::package::{ExtData, VsdxRest};
use diacraft_doc::{Ext, Held, HeldStyleValue, Kept, OdfListLevel, OdfPage, OdfParagraph, OdfRow, OdfRun, OdfShape, OdfStyle, OdfText, OdfTextStyle};

/// The elements a copy of a value makes: the value and each element of each list it holds, counted down through the lists those hold.
pub(crate) trait Weigh {
    fn weigh(&self) -> u64;
}

fn count<T>(list: &[T]) -> u64 {
    u64::try_from(list.len()).unwrap_or(u64::MAX)
}

fn sum<'a, T: Weigh + 'a>(list: impl IntoIterator<Item = &'a T>) -> u64 {
    list.into_iter().fold(0u64, |total, item| total.saturating_add(item.weigh()))
}

impl Weigh for Kept {
    /// The attributes and the nodes are shared lists: a copy is two counts.
    fn weigh(&self) -> u64 {
        let Kept { raw: _, kept: _ } = self;
        1
    }
}

impl Weigh for VsdxRest {
    fn weigh(&self) -> u64 {
        let VsdxRest(entries) = self;
        1u64.saturating_add(count(entries))
    }
}

impl Weigh for Held {
    fn weigh(&self) -> u64 {
        let Held { group: _, stamp: _, attrs: _, kept: _ } = self;
        1
    }
}

impl Weigh for HeldStyleValue {
    fn weigh(&self) -> u64 {
        let HeldStyleValue { group: _, stamp: _, attrs: _, kept: _ } = self;
        1
    }
}

impl Weigh for OdfShape {
    fn weigh(&self) -> u64 {
        let OdfShape { kept, style_name: _, held, units: _ } = self;
        kept.weigh().saturating_add(sum(held))
    }
}

impl Weigh for OdfRow {
    fn weigh(&self) -> u64 {
        let OdfRow { kept, style_name: _, held } = self;
        kept.weigh().saturating_add(sum(held))
    }
}

impl Weigh for OdfPage {
    fn weigh(&self) -> u64 {
        let OdfPage { kept, master_page: _, layout: _, master_page_attrs: _, layout_props: _, drawing_page_style: _, units: _, stamp: _ } = self;
        kept.weigh()
    }
}

impl Weigh for OdfStyle {
    fn weigh(&self) -> u64 {
        let OdfStyle { kept, name: _, display_name: _, parent: _, family: _, unmapped: _, units: _, held, text_held } = self;
        kept.weigh().saturating_add(sum(held)).saturating_add(sum(text_held))
    }
}

impl Weigh for OdfTextStyle {
    fn weigh(&self) -> u64 {
        let OdfTextStyle { name: _, stamp: _, held } = self;
        1u64.saturating_add(sum(held))
    }
}

impl Weigh for OdfRun {
    fn weigh(&self) -> u64 {
        let OdfRun { from: _, to: _, style, element: _ } = self;
        1u64.saturating_add(style.weigh())
    }
}

impl Weigh for OdfListLevel {
    fn weigh(&self) -> u64 {
        let OdfListLevel { list: _, item: _ } = self;
        1
    }
}

impl Weigh for OdfParagraph {
    fn weigh(&self) -> u64 {
        let OdfParagraph { style, characters: _, runs, element: _, levels } = self;
        1u64.saturating_add(style.weigh()).saturating_add(sum(runs)).saturating_add(sum(levels))
    }
}

impl Weigh for OdfText {
    /// The paragraphs are a list the copy makes again, with each paragraph's runs and levels and the values its styles hold.
    fn weigh(&self) -> u64 {
        let OdfText { kept, paragraphs } = self;
        kept.weigh().saturating_add(sum(paragraphs))
    }
}

impl<T: Weigh> Weigh for ExtData<T> {
    /// A slot's three sides: the other format's data, the VSDX elements the object had no place for, and what a newer build wrote.
    fn weigh(&self) -> u64 {
        let ExtData { odf, vsdx, native } = self;
        odf.weigh().saturating_add(vsdx.weigh()).saturating_add(native.weigh())
    }
}

/// What copying a slot costs, or nothing for a slot that holds nothing.
pub(crate) fn slot<T: Weigh + Default + Clone + PartialEq>(slot: &Ext<T>) -> u64 {
    slot.get().map_or(0, Weigh::weigh)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn a_slot_is_sized_by_the_elements_of_the_lists_it_holds() {
        assert_eq!(slot(&Ext::<OdfShape>::default()), 0, "no slot costs nothing");
        let mut shape: Ext<OdfShape> = Ext::default();
        shape.make_mut().odf.style_name = Some(Arc::from("gr1"));
        assert_eq!(slot(&shape), 3, "the shape's own data, the VSDX side and the unknown side");
        for _ in 0..4 {
            shape.make_mut().odf.held.push(Held {
                group: diacraft_doc::HeldGroup::Fill,
                stamp: None,
                attrs: Default::default(),
                kept: Default::default(),
            });
        }
        shape.make_mut().vsdx.0.push((Arc::from("k"), Kept::default()));
        assert_eq!(slot(&shape), 1 + 4 + (1 + 1) + 1, "each held value and each entry of the VSDX side");
    }

    #[test]
    fn the_paragraphs_of_a_text_are_sized_with_their_runs_levels_and_values() {
        let mut text: Ext<OdfText> = Ext::default();
        let paragraph = OdfParagraph {
            style: OdfTextStyle {
                name: None,
                stamp: None,
                held: vec![
                    HeldStyleValue { group: diacraft_doc::TextGroup::Size, stamp: None, attrs: Default::default(), kept: Default::default() };
                    2
                ],
            },
            characters: None,
            runs: vec![OdfRun { from: 0, to: 1, style: OdfTextStyle::default(), element: None }; 3],
            element: None,
            levels: vec![OdfListLevel::default(); 2],
        };
        // 1 paragraph + (1 + 2 values) of its style + 3 runs of (1 + 1) each + 2 levels.
        assert_eq!(paragraph.weigh(), 1 + 3 + 6 + 2);
        text.make_mut().odf.paragraphs = vec![paragraph; 5];
        assert_eq!(slot(&text), (1 + 5 * 12) + 1 + 1);
    }
}
