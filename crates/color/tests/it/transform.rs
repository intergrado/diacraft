//! DrawingML-style colour transforms (the theme part of a document is DrawingML).

use diacraft_color::{ColorTransform, Rgba, apply};

#[test]
fn transforms() {
    let c = Rgba::rgb(0x40, 0x80, 0xC0);
    assert_eq!(apply(c, &[ColorTransform::Alpha(50000)]).a, 128);
    let lighter = apply(c, &[ColorTransform::LumMod(20000), ColorTransform::LumOff(80000)]);
    assert!(lighter.luminance() > c.luminance());
    let darker = apply(c, &[ColorTransform::LumMod(50000)]);
    assert!(darker.luminance() < c.luminance());
    assert_eq!(apply(Rgba::WHITE, &[ColorTransform::Shade(0)]), Rgba::BLACK);
    assert_eq!(apply(Rgba::BLACK, &[ColorTransform::Tint(0)]), Rgba::WHITE);
    assert_eq!(apply(c, &[ColorTransform::Inv]), Rgba::rgb(0xBF, 0x7F, 0x3F));
}

/// Hostile values never panic, and the alpha stays a byte.
#[test]
fn hostile_values_give_a_colour() {
    let c = Rgba::rgb(0x40, 0x80, 0xC0);
    for v in [i32::MAX, i32::MIN, 0, -1, 1] {
        for t in [
            ColorTransform::Tint(v),
            ColorTransform::Shade(v),
            ColorTransform::LumMod(v),
            ColorTransform::LumOff(v),
            ColorTransform::SatMod(v),
            ColorTransform::SatOff(v),
            ColorTransform::HueMod(v),
            ColorTransform::HueOff(v),
            ColorTransform::Alpha(v),
            ColorTransform::AlphaMod(v),
            ColorTransform::AlphaOff(v),
            ColorTransform::RedMod(v),
            ColorTransform::GreenMod(v),
            ColorTransform::BlueMod(v),
        ] {
            let _ = apply(c, &[t]);
        }
    }
}

/// Transforms have an XML name and come back from it, with the value in file units.
#[test]
fn a_transform_round_trips_through_its_xml_name() {
    for t in [ColorTransform::LumMod(75000), ColorTransform::Tint(40000), ColorTransform::Alpha(0), ColorTransform::Comp, ColorTransform::Gray] {
        assert_eq!(ColorTransform::from_xml(t.xml_name(), t.value()), Some(t));
    }
    assert_eq!(ColorTransform::from_xml("nonsense", Some(1)), None);
}
