//! Names, widths, band classes and the read-back of the markers of the line ends.

use super::*;

fn key(number: u32, band: u32) -> Key {
    Key { number, band }
}

/// The definition of a marker: what the plumbing of a save names.
fn def(number: u32, band: u32) -> Def {
    Def::Marker(key(number, band))
}

#[test]
fn a_name_is_the_prefix_the_number_and_the_band_suffix_and_is_read_back_as_the_number_and_the_band() {
    assert_eq!(key(13, 0).name(), "DiaCraft_20_end_20_13");
    assert_eq!(key(16, 23).name(), "DiaCraft_20_end_20_16_20_t23");
    assert_eq!(key(13, 0).display_name(), "DiaCraft end 13");
    assert_eq!(key(16, 23).display_name(), "DiaCraft end 16 t23");
    for n in 1..=45 {
        assert_eq!(key_of(&key(n, 0).name()), Some(key(n, 0)));
        for k in 1..=99 {
            assert_eq!(key_of(&key(n, k).name()), Some(key(n, k)));
        }
    }
    for bad in [
        "DiaCraft_20_end_20_013",
        "DiaCraft_20_end_20_+4",
        "DiaCraft_20_end_20_0",
        "DiaCraft_20_end_20_46",
        "Arrow",
        "DiaCraft_20_end_20_",
        "DiaCraft_20_end_20_16_20_t023",
        "DiaCraft_20_end_20_16_20_t0",
        "DiaCraft_20_end_20_16_20_t100",
        "DiaCraft_20_end_20_16_20_T23",
        "DiaCraft_20_end_20_16_20_t",
        "DiaCraft_20_end_20_16_20_t+5",
        "DiaCraft_20_end_20_16_20_t-5",
        "DiaCraft_20_end_20_16_t23",
        "DiaCraft_20_end_20_16_20_t23_20_t23",
    ] {
        assert_eq!(key_of(bad), None, "{bad}");
    }
}

#[test]
fn the_band_class_is_a_hundred_times_the_weight_over_s_kept_from_1_to_99() {
    // 0.75 pt at class 2 (S 3.27): 0.2294, class 23. 5 pt: S 7.52, 0.6649, class 66. Weight 0: 0.25 pt over the base alone: 0.0992, class 10.
    // 2 pt: 2 / 4.52 = 0.4425, class 44.
    assert_eq!(plan(16, 2, 0.75).unwrap().key, key(16, 23));
    assert_eq!(plan(16, 2, 5.0).unwrap().key, key(16, 66));
    assert_eq!(plan(16, 2, 0.0).unwrap().key, key(16, 10));
    assert_eq!(plan(14, 2, 2.0).unwrap().key, key(14, 44));
    assert_eq!(plan(14, 2, 0.75).unwrap().key, key(14, 23));
    // Kept between 1 and 99: a hair-thin line on the largest class is class 1, and a heavy one on the smallest is 99.
    assert_eq!(plan(16, 6, 0.001).unwrap().key.band, 1);
    assert_eq!(plan(16, 0, 1000.0).unwrap().key.band, 99);
    // A mark with no band is the same marker at every weight and class.
    for (n, w, c) in [(4, 0.0, 0), (4, 0.75, 2), (4, 9.0, 6), (13, 2.0, 3)] {
        assert_eq!(plan(n, c, w).unwrap().key, key(n, 0), "{n}");
    }
}

#[test]
fn the_width_is_the_spread_times_the_class_base_plus_the_weight() {
    let w = |n: u32, c: u32, pt: f64| plan(n, c, pt).unwrap().width;
    let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-12, "{a} against {b}");
    // Number 13, class 2, 0.75 pt: 2 x (2.52 + 0.75) = 6.54 pt. Class 4: 2 x (3.96 + 0.75) = 9.42 pt.
    close(w(13, 2, 0.75), 6.54);
    close(w(13, 4, 0.75), 9.42);
    // 34 is 22's box, 2.24 x 3.27; 35 is 1.5 x 3.27; 34 at size 3: 2.24 x 3.99; 8 is 4's box, 2 x 3.27.
    close(w(34, 2, 0.75), 7.3248);
    close(w(35, 2, 0.75), 4.905);
    close(w(34, 3, 0.75), 8.9376);
    close(w(8, 2, 0.75), 6.54);
    close(w(44, 2, 0.75), 6.54);
    // Class 6 at 1 pt: 2 x (18 + 1) = 38 pt.
    close(w(13, 6, 1.0), 38.0);
    // A class outside 0 to 6 is refused in words, and so is a number outside 1 to 45.
    assert_eq!(plan(4, 7, 0.75), Err(WORDS_SIZE));
    assert_eq!(plan(4, 100, 0.75), Err(WORDS_SIZE));
    assert_eq!(plan(46, 2, 0.75), Err(WORDS_NUMBER));
    assert_eq!(plan(0, 2, 0.75), Err(WORDS_NUMBER));
    // 254 is the custom end: refused in words of its own.
    assert!(plan(254, 2, 0.75).unwrap_err().contains("custom line end"));
}

#[test]
fn every_number_has_a_plan_and_art_in_the_box_of_its_spread() {
    let mut steps = 0;
    for n in 1..=45 {
        let p = plan(n, 2, 0.75).unwrap();
        let a = art(p.key, &mut steps).unwrap();
        assert!(a.d.starts_with("M ") && a.d.ends_with('Z'), "{n}: {}", a.d);
        let view: Vec<f64> = a.view_box.split(' ').map(|v| v.parse().unwrap()).collect();
        // The width of the box is the spread, in thousandths of `S`, and the marker's width is that many thousandths of `S`.
        assert!((view[2] / 1000.0 * 3.27 - p.width).abs() < 0.005, "{n}: {} against {}", view[2], p.width);
        assert_eq!(a.centred, matches!(n, 9 | 10 | 11 | 20 | 21 | 42), "{n}");
    }
    assert!(steps > 0);
}

#[test]
fn a_width_is_written_in_points_to_four_places() {
    assert_eq!(points_text(6.54), "6.54pt");
    assert_eq!(points_text(9.42), "9.42pt");
    assert_eq!(points_text(2.0 * (2.52 + 0.026_458_333_333 * 72.0)), "8.85pt");
    assert_eq!(points_text(10.0), "10pt");
    assert_eq!(points_text(7.3248), "7.3248pt");
}

#[test]
fn the_element_names_the_marker_gives_its_display_name_and_holds_its_form() {
    let mut steps = 0;
    let a = art(key(4, 0), &mut steps).unwrap();
    assert_eq!(
        element(key(4, 0), &a, "draw", "svg", ""),
        "<draw:marker draw:name=\"DiaCraft_20_end_20_4\" draw:display-name=\"DiaCraft end 4\" svg:viewBox=\"0 0 2000 2000\" svg:d=\"M 1000 0 L 2000 2000 L 0 2000 Z\"/>"
    );
    // A new marker for 14, size 2, 0.75 pt, has its display name written.
    let k = plan(14, 2, 0.75).unwrap().key;
    let e = element(k, &art(k, &mut steps).unwrap(), "draw", "svg", "");
    assert!(e.contains("draw:name=\"DiaCraft_20_end_20_14_20_t23\" draw:display-name=\"DiaCraft end 14 t23\""), "{e}");
}

/// A `styles.xml` whose `office:styles` holds `inner`.
fn styles_with(inner: &str) -> Vec<u8> {
    format!(
        r#"<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"><office:styles>{inner}</office:styles></office:document-styles>"#
    )
    .into_bytes()
}

#[test]
fn the_read_back_of_the_markers_written_refuses_what_is_not_as_written() {
    let four: BTreeSet<Def> = [def(4, 0)].into_iter().collect();
    let a = art(key(4, 0), &mut 0).unwrap();
    let ok = element(key(4, 0), &a, "draw", "svg", "");
    let none = BTreeSet::new();
    let run = |inner: &str, needs: &BTreeSet<Def>, wrote: &BTreeSet<Def>, removed: &BTreeSet<Def>| {
        check(&styles_with(inner), needs, wrote, removed, &Limits::default(), &mut WriteWork::default())
    };
    assert!(run(&ok, &four, &four, &none).is_ok(), "the marker as written");
    assert!(run("", &none, &none, &none).is_ok(), "nothing asked for");
    let why = |r: Result<(), WriteError>| match r {
        Err(WriteError::SelfCheck(words)) => words,
        other => panic!("expected a refusal of the check: {other:?}"),
    };
    assert!(why(run("", &four, &four, &none)).contains("not in office:styles"));
    assert!(why(run("", &four, &none, &none)).contains("not in office:styles"), "one the file held is there too");
    assert!(why(run(&ok.replace("L 2000 2000", "L 1000 1000"), &four, &four, &none)).contains("does not draw the form"), "another path");
    assert!(why(run(&ok.replace("0 0 2000 2000", "0 0 20 20"), &four, &four, &none)).contains("does not draw the form"), "another box");
    assert!(why(run(&format!("{ok}{ok}"), &four, &four, &none)).contains("twice"));
    // A marker the file already held is DiaCraft's by its name, and used as it is: another path, another box, a twin.
    assert!(run(&ok.replace("L 2000 2000", "L 1000 1000"), &four, &none, &none).is_ok(), "another path, held");
    assert!(run(&ok.replace("0 0 2000 2000", "0 0 20 20"), &four, &none, &none).is_ok(), "another box, held");
    assert!(run(&format!("{ok}{ok}"), &four, &none, &none).is_ok(), "a twin, held");
    // A marker of another name is not DiaCraft's, and one of another number or band is not the one asked for.
    let other = element(key(13, 0), &art(key(13, 0), &mut 0).unwrap(), "draw", "svg", "");
    assert!(why(run(&other, &four, &four, &none)).contains("not in office:styles"));
    assert!(run(&format!("{ok}{other}"), &four, &four, &none).is_ok(), "a marker beside it that was not asked for is none of the check's");
    let banded: BTreeSet<Def> = [def(16, 23)].into_iter().collect();
    let one = element(key(16, 23), &art(key(16, 23), &mut 0).unwrap(), "draw", "svg", "");
    let other_band = element(key(16, 44), &art(key(16, 44), &mut 0).unwrap(), "draw", "svg", "");
    assert!(run(&one, &banded, &banded, &none).is_ok());
    assert!(why(run(&other_band, &banded, &banded, &none)).contains("not in office:styles"), "another band class is another marker");
    // Only the box or only the path being right is not enough.
    assert!(run(&ok.replace("0 0 2000 2000", "0 0 2000 3000"), &four, &four, &none).is_err());
    // A marker the save took out is gone: one still there is refused, and one that is asked for again is left to the other rule.
    let gone: BTreeSet<Def> = [def(13, 0)].into_iter().collect();
    assert!(run(&ok, &none, &none, &gone).is_ok(), "13 is not there");
    assert!(why(run(&format!("{ok}{other}"), &none, &none, &gone)).contains("no style names"));
}

#[test]
fn a_size_class_is_a_whole_number_from_0_to_100_and_a_weight_a_length_or_the_default() {
    for (v, class) in
        [(0.0, Some(0)), (2.0, Some(2)), (5.0, Some(5)), (6.0, Some(6)), (100.0, Some(100)), (101.0, None), (-1.0, None), (1.5, None), (0.5, None)]
    {
        assert_eq!(class_of(v), class, "{v}");
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.5, 100.5] {
        assert_eq!(class_of(bad), None, "{bad}");
    }
    // The weight is in inches: a hundredth of an inch is 0.72 pt; none, a negative one and one that is no length are the root style's 0.75 pt.
    assert!((weight_pt(Some(0.01)).unwrap() - 0.72).abs() < 1e-12);
    assert_eq!(weight_pt(Some(0.0)), Ok(0.0));
    assert_eq!(weight_pt(None), Ok(0.75));
    for bad in [-0.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(weight_pt(Some(bad)), Ok(0.75), "{bad}");
    }
}

/// The size class of an end is 2 where the cell is none, the cell's class, or the words of a refusal for a number no class is.
#[test]
fn the_size_class_of_a_cell_is_2_for_none_the_cells_class_or_a_refusal() {
    assert_eq!(class_of_cell(None), Ok(2));
    for (v, class) in [(0.0, 0), (2.0, 2), (6.0, 6), (7.0, 7), (100.0, 100)] {
        assert_eq!(class_of_cell(Some(v)), Ok(class), "{v}");
    }
    for bad in [101.0, -1.0, 1.5, f64::NAN, f64::INFINITY] {
        assert_eq!(class_of_cell(Some(bad)), Err(WORDS_SIZE), "{bad}: a cell that is no class is not taken for 2");
    }
}

/// A weight that is no number from 0 to a million points is refused in words, by the helper at the cap (`<=`, not `<`) and by
/// `plan` for both a number with a band and one without, so no width is `inf` or hundreds of digits and no band class is a NaN.
#[test]
fn a_weight_past_a_million_points_is_refused_and_the_cap_itself_is_written() {
    assert_eq!(MOST_WEIGHT_PT, 1_000_000.0);
    assert!(weight_in_range(0.0) && weight_in_range(0.75) && weight_in_range(MOST_WEIGHT_PT));
    assert!(!weight_in_range(MOST_WEIGHT_PT.next_up()) && !weight_in_range((-0.0_f64).next_down()) && !weight_in_range(f64::NAN));
    assert!(!weight_in_range(f64::INFINITY) && !weight_in_range(f64::NEG_INFINITY) && !weight_in_range(f64::MAX) && !weight_in_range(1e300));
    // The cell's weight in inches: past the cap in points is refused, and the product that overflows to an infinity too.
    assert_eq!(weight_pt(Some(MOST_WEIGHT_PT / 72.0 * 1.001)), Err(WORDS_WEIGHT));
    for inches in [1e5, 1e300, 3e307, f64::MAX / 72.0, f64::MAX] {
        assert_eq!(weight_pt(Some(inches)), Err(WORDS_WEIGHT), "{inches} in");
    }
    assert!((weight_pt(Some(MOST_WEIGHT_PT / 72.0 * 0.999)).unwrap() - MOST_WEIGHT_PT * 0.999).abs() < 1e-6);
    for number in [16, 4, 13, 22, 45] {
        for class in [0, 2, 6] {
            for bad in [MOST_WEIGHT_PT.next_up(), 7.2e6, 1e300, f64::MAX, f64::MAX / 72.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -0.5] {
                assert_eq!(plan(number, class, bad), Err(WORDS_WEIGHT), "{number} class {class} at {bad} pt");
            }
            let at_cap = plan(number, class, MOST_WEIGHT_PT).unwrap();
            assert!(at_cap.width.is_finite() && at_cap.width < 2_300_000.0, "{number} class {class}: {}", at_cap.width);
            let text = points_text(at_cap.width);
            assert!(text.ends_with("pt") && text.trim_end_matches("pt").chars().all(|c| c.is_ascii_digit() || c == '.'), "{text}");
            assert!(text.len() <= "2240040.3200pt".len(), "{text}");
            assert!(plan(number, class, 0.0).is_ok());
        }
    }
    // A band class is the one of the weight at the cap: 99 for a mark with a band, on every class.
    assert_eq!(plan(16, 6, MOST_WEIGHT_PT).unwrap().key.band, 99);
    // The words of the other refusals come first: a class the specification gives no base for is its own refusal, whatever the weight.
    assert_eq!(plan(16, 7, 1e300), Err(WORDS_SIZE));
    assert_eq!(plan(46, 2, 1e300), Err(WORDS_NUMBER));
    assert!(plan(254, 2, 1e300).unwrap_err().contains("custom line end"));
}
