//! Line ends read back by number and size class: a marker of DiaCraft's own
//! name gives its number, and the size class comes from the width, the line's weight and the number's spread, within 0.1 pt of a class's width.

use crate::drawing::{Drawing, cell, close, num, only};
use diacraft_odg::ReadReport;

/// One rectangle whose style is the automatic style `gr1` with these graphic properties.
fn read(common: &str, props: &str) -> (diacraft_doc::Document, ReadReport) {
    Drawing::a4()
        .common(common)
        .auto(&format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties {props}/></style:style>"#))
        .page(r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2.5cm"/>"#)
        .read()
}

/// A marker element of this name, of a path nothing reads.
fn marker(name: &str) -> String {
    format!(r#"<draw:marker draw:name="{name}" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#)
}

/// The end of the one shape: its number and size class, and how many widths were kept.
fn end_of(name: &str, weight: &str, width: &str) -> (f64, f64, u64) {
    let (doc, report) = read(&marker(name), &format!(r#"svg:stroke-width="{weight}" draw:marker-end="{name}" draw:marker-end-width="{width}""#));
    let s = only(&doc);
    (num(cell(s, "EndArrow")), num(cell(s, "EndArrowSize")), report.kept("marker_width"))
}

#[test]
fn odg_styles_a_marker_of_diacraft_s_name_gives_its_number_and_the_class_its_width_says() {
    // `EndArrow` 13 at 9.42 pt and 0.75 pt: 2 x (3.96 + 0.75) is class 4.
    assert_eq!(end_of("DiaCraft_20_end_20_13", "0.75pt", "9.42pt"), (13.0, 4.0, 0));
    // 9.62 pt is 0.2 pt from class 4's width and farther from every other: the stand-in 2, and the width kept.
    assert_eq!(end_of("DiaCraft_20_end_20_13", "0.75pt", "9.62pt"), (13.0, 2.0, 1));
    // The name DiaCraft writes has no leading zero: `013` is another producer's, the stand-in 4, and the marker is kept.
    let (doc, report) = read(
        &marker("DiaCraft_20_end_20_013"),
        r#"svg:stroke-width="0.75pt" draw:marker-end="DiaCraft_20_end_20_013" draw:marker-end-width="9.42pt""#,
    );
    assert_eq!((num(cell(only(&doc), "EndArrow")), num(cell(only(&doc), "EndArrowSize"))), (4.0, 2.0));
    assert_eq!(report.kept("marker"), 1);
    // `DiaCraft_20_end_20_8` at 6.54 pt: gives 8 4's box, a spread of 2 (2 x 3.27): number 8, class 2. At 9 pt the nearest class, 4 at 9.42 pt, is 0.42 pt
    // away: class 2 as the stand-in, and the width kept.
    assert_eq!(end_of("DiaCraft_20_end_20_8", "0.75pt", "6.54pt"), (8.0, 2.0, 0));
    assert_eq!(end_of("DiaCraft_20_end_20_8", "0.75pt", "9pt"), (8.0, 2.0, 1));
}

#[test]
fn odg_styles_a_band_suffix_is_read_as_the_number_and_a_badly_spelt_one_is_another_producer_s_marker() {
    for (name, number) in [
        ("DiaCraft_20_end_20_16_20_t23", 16.0),
        ("DiaCraft_20_end_20_4_20_t23", 4.0),
        ("DiaCraft_20_end_20_45_20_t99", 45.0),
        ("DiaCraft_20_end_20_1_20_t1", 1.0),
    ] {
        let (doc, report) = read(&marker(name), &format!(r#"svg:stroke-width="0.75pt" draw:marker-end="{name}" draw:marker-end-width="6.54pt""#));
        assert_eq!(num(cell(only(&doc), "EndArrow")), number, "{name}");
        assert_eq!(num(cell(only(&doc), "EndArrowSize")), 2.0, "{name}");
        assert_eq!(report.kept("marker"), 0, "{name}");
    }
    for name in [
        "DiaCraft_20_end_20_16_20_t023",
        "DiaCraft_20_end_20_16_20_t0",
        "DiaCraft_20_end_20_16_20_t100",
        "DiaCraft_20_end_20_16_20_T23",
        "DiaCraft_20_end_20_16_20_t",
        "DiaCraft_20_end_20_16_20_t+23",
        "DiaCraft_20_end_20_16_20_t23x",
        "DiaCraft_20_end_20_16_t23",
    ] {
        let (doc, report) = read(&marker(name), &format!(r#"svg:stroke-width="0.75pt" draw:marker-end="{name}" draw:marker-end-width="6.54pt""#));
        // The stand-in, a solid triangle of size 2, and the marker kept.
        assert_eq!((num(cell(only(&doc), "EndArrow")), num(cell(only(&doc), "EndArrowSize"))), (4.0, 2.0), "{name}");
        assert_eq!(report.kept("marker"), 1, "{name}");
    }
}

#[test]
fn odg_styles_the_numbers_no_source_measures_read_back_in_the_box_of_dia_craft_s_rule() {
    // At weight 0.75 pt: 34 at size 2 is 2.24 x 3.27 = 7.3248 pt, 35 is 1.5 x 3.27 = 4.905 pt, 34 at size 3 is 2.24 x 3.99 = 8.9376 pt.
    for (name, width, number, class) in [
        ("DiaCraft_20_end_20_34_20_t23", "7.3248pt", 34.0, 2.0),
        ("DiaCraft_20_end_20_35_20_t23", "4.905pt", 35.0, 2.0),
        ("DiaCraft_20_end_20_34_20_t23", "8.9376pt", 34.0, 3.0),
        ("DiaCraft_20_end_20_44_20_t23", "6.54pt", 44.0, 2.0),
        ("DiaCraft_20_end_20_45_20_t23", "6.54pt", 45.0, 2.0),
        ("DiaCraft_20_end_20_8", "6.54pt", 8.0, 2.0),
        ("DiaCraft_20_end_20_19_20_t23", "6.54pt", 19.0, 2.0),
        ("DiaCraft_20_end_20_18_20_t23", "6.6054pt", 18.0, 2.0),
    ] {
        assert_eq!(end_of(name, "0.75pt", width), (number, class, 0), "{name} at {width}");
    }
}

#[test]
fn odg_styles_class_6_has_a_base_of_18_pt() {
    // `EndArrow` 13, class 6, 1 pt: 2 x (18 + 1) = 38 pt, and class 5 would be 2 x (9 + 1) = 20 pt, 18 pt away.
    assert_eq!(end_of("DiaCraft_20_end_20_13", "1pt", "38pt"), (13.0, 6.0, 0));
    assert_eq!(end_of("DiaCraft_20_end_20_13", "1pt", "20pt"), (13.0, 5.0, 0));
    // Every class at a weight, for a number that spreads 1.5 and one that spreads 2.24.
    for (n, spread) in [(10u32, 1.5), (22, 2.24), (5, 1.99)] {
        for (class, base) in [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0].into_iter().enumerate() {
            let name = format!("DiaCraft_20_end_20_{n}");
            let width = format!("{}pt", spread * (base + 2.0));
            assert_eq!(end_of(&name, "2pt", &width), (f64::from(n), class as f64, 0), "{n} at class {class}");
        }
    }
}

#[test]
fn odg_styles_the_widths_libre_office_rounds_to_a_thousandth_of_a_centimetre_still_read_as_their_class() {
    // A style LibreOffice saved from the file: 0.026 cm (0.737008 pt) and 0.332 cm (9.411024 pt): class 4 predicts 9.394016 pt, 0.017 pt away.
    assert_eq!(end_of("DiaCraft_20_end_20_13", "0.026cm", "0.332cm"), (13.0, 4.0, 0));
    // The widest spread, 2.24, is the largest rounding: both the weight and the width are off by half a thousandth of a centimetre in the worst direction.
    let weight_cm: f64 = 0.026;
    let weight_pt = weight_cm / 2.54 * 72.0;
    for (n, spread) in [(22u32, 2.24f64), (13, 2.0), (10, 1.5)] {
        for (class, base) in [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0].into_iter().enumerate() {
            let exact_cm = spread * (base + weight_pt) / 72.0 * 2.54;
            for off in [-0.0005, 0.0, 0.0005] {
                let width = format!("{:.4}cm", (exact_cm * 1000.0).round() / 1000.0 + off);
                let name = format!("DiaCraft_20_end_20_{n}");
                assert_eq!(end_of(&name, "0.026cm", &width), (f64::from(n), class as f64, 0), "{n} class {class} at {width}");
            }
        }
    }
}

#[test]
fn odg_styles_a_width_no_class_is_within_a_tenth_of_a_point_of_is_the_stand_in_with_the_width_kept() {
    // The same weight and 0.3 cm (8.503937 pt): class 3 predicts 7.954016 pt and class 4 9.394016 pt, both more than 0.1 pt away.
    assert_eq!(end_of("DiaCraft_20_end_20_13", "0.026cm", "0.3cm"), (13.0, 2.0, 1));
}

#[test]
fn a_width_is_a_class_up_to_a_tenth_of_a_point_either_way_and_no_class_past_it() {
    // Number 13 at 0.75 pt: class 3 is 2 x (3.24 + 0.75) = 7.98 pt.
    for (width, class, kept) in [
        ("7.98pt", 3.0, 0),
        ("8.07pt", 3.0, 0),
        ("7.89pt", 3.0, 0),
        ("8.0799pt", 3.0, 0),
        ("7.8801pt", 3.0, 0),
        ("8.081pt", 2.0, 1),
        ("8.09pt", 2.0, 1),
        ("7.879pt", 2.0, 1),
        ("7.8pt", 2.0, 1),
    ] {
        let got = end_of("DiaCraft_20_end_20_13", "0.75pt", width);
        assert_eq!((got.0, got.1, got.2), (13.0, class, kept), "{width}");
    }
    // A line with no width attribute for its marker has no class to find: the stand-in, and the width kept.
    let (doc, report) = read(&marker("DiaCraft_20_end_20_13"), r#"svg:stroke-width="0.75pt" draw:marker-end="DiaCraft_20_end_20_13""#);
    assert_eq!(num(cell(only(&doc), "EndArrowSize")), 2.0);
    assert_eq!(report.kept("marker_width"), 1);
    close(num(cell(only(&doc), "EndArrow")), 13.0);
}
