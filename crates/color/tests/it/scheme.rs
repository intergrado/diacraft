//! The twelve theme colour slots and the mapped aliases.

use diacraft_color::{ColorScheme, Rgba, SchemeSlot};

#[test]
fn scheme_slots() {
    let mut s = ColorScheme { name: "t".into(), colors: [Rgba::BLACK; 12] };
    s.set(SchemeSlot::Accent3, Rgba::WHITE);
    assert_eq!(s.get(SchemeSlot::Accent3), Rgba::WHITE);
    assert_eq!(s.get(SchemeSlot::Tx1), Rgba::BLACK);
    for slot in SchemeSlot::THEME {
        assert_eq!(SchemeSlot::from_xml(slot.xml_name()), Some(slot));
    }
}

/// The aliases read the slot they name: `bg1` is `lt1`, `tx1` is `dk1`, `bg2` is `lt2`, `tx2` is `dk2`.
#[test]
fn aliases_read_their_slot() {
    let mut colors = [Rgba::BLACK; 12];
    for (i, c) in colors.iter_mut().enumerate() {
        *c = Rgba::rgb(i as u8, 0, 0);
    }
    let s = ColorScheme { name: "t".into(), colors };
    assert_eq!(s.get(SchemeSlot::Bg1), s.get(SchemeSlot::Lt1));
    assert_eq!(s.get(SchemeSlot::Tx1), s.get(SchemeSlot::Dk1));
    assert_eq!(s.get(SchemeSlot::Bg2), s.get(SchemeSlot::Lt2));
    assert_eq!(s.get(SchemeSlot::Tx2), s.get(SchemeSlot::Dk2));
    assert_eq!(SchemeSlot::from_xml("phClr"), Some(SchemeSlot::PhClr));
    assert_eq!(SchemeSlot::from_xml("accent7"), None);
}
