//! Shapes, order, groups, flips, turns and scale.

use std::f64::consts::FRAC_PI_2;

use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

fn filled(sb: ShapeBuilder, color: &str) -> ShapeBuilder {
    sb.cell("FillForegnd", color)
}

#[test]
fn a_rectangle_draws_its_fill_and_its_outline() {
    let sb = filled(rect(10, 1.0, 1.0, 2.0, 2.0), "#FF0000").cell("LineWeight", "0.1").cell("LineColor", "#000000");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_red(2.0, 2.0), "the inside is the fill");
    assert!(pic.is_white(0.5, 0.5), "outside is bare paper");
    assert!(pic.is_dark(1.0, 2.0) && pic.is_dark(3.0, 2.0), "the outline is on the edges, centred on them");
    assert!(pic.is_dark(2.0, 1.0) && pic.is_dark(2.0, 3.0));
}

#[test]
fn the_page_is_drawn_y_up_so_a_shape_low_on_the_page_is_low_on_the_screen() {
    let pic = draw(&one_page(vec![filled(rect(1, 0.5, 0.5, 1.0, 1.0), "#FF0000")]));
    // The page is 4 inches tall at 100 pixels to the inch: y = 1 inch up is 300 pixels down.
    assert!(pic.is_red(1.0, 1.0));
    assert!(pic.is_white(1.0, 3.0));
    let p = pic.r.rgba_premul.chunks(4).nth(300 * 400 + 100).unwrap();
    assert_eq!(p[0], 255, "pixel (100, 300) is red, so y = 1 inch is 300 pixels from the top");
}

#[test]
fn a_later_shape_is_in_front_of_an_earlier_one() {
    let a = filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000");
    let b = filled(rect(2, 2.0, 2.0, 1.5, 1.5), "#0000FF");
    let pic = draw(&one_page(vec![a, b]));
    assert!(pic.is_red(1.5, 1.5));
    assert!(pic.is(2.5, 2.5, (0, 0, 255), 6), "the later shape covers the overlap");
    let pic2 = draw(&one_page(vec![filled(rect(2, 2.0, 2.0, 1.5, 1.5), "#0000FF"), filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000")]));
    assert!(pic2.is_red(2.5, 2.5), "swapped, the red one is in front: the order is the file's, not the IDs'");
}

#[test]
fn a_member_takes_its_groups_frame_and_a_turned_group_turns_its_members() {
    let child = filled(rect(2, 0.5, 0.5, 1.0, 1.0), "#FF0000");
    let group = shape_at(1, 1.0, 1.0, 2.0, 2.0).group().child(child);
    let pic = draw(&one_page(vec![group]));
    assert!(pic.is_red(2.0, 2.0), "a member at (0.5, 0.5) of a group at (1, 1) is at (1.5, 1.5) to (2.5, 2.5)");
    assert!(pic.is_white(1.2, 1.2));
    // The group turned a quarter turn about its pin at (3, 1): the member's box [0.5, 1.5]^2 goes to x in [1.5, 2.5], y in [1.5, 2.5].
    let turned = shape_at(1, 3.0, 1.0, 2.0, 2.0).cell("Angle", &FRAC_PI_2.to_string()).group().child(filled(rect(2, 0.5, 0.5, 1.0, 1.0), "#FF0000"));
    let pic = draw(&one_page(vec![turned]));
    assert!(pic.is_red(2.0, 2.0), "the turn is counterclockwise about the pin");
    assert!(pic.is_white(3.5, 1.5), "where an unturned member would be");
}

#[test]
fn a_flip_mirrors_the_shape_about_its_pin() {
    let triangle = |flip_x: &str, flip_y: &str| {
        let sb = shape_at(1, 1.0, 1.0, 2.0, 2.0)
            .cell("FlipX", flip_x)
            .cell("FlipY", flip_y)
            .cell("PinX", "2")
            .cell("PinY", "2")
            .cell("LocPinX", "1")
            .cell("LocPinY", "1");
        filled(
            geometry_of(
                sb,
                vec![
                    ("MoveTo", vec![("X", 0.0), ("Y", 0.0)]),
                    ("LineTo", vec![("X", 2.0), ("Y", 0.0)]),
                    ("LineTo", vec![("X", 0.0), ("Y", 2.0)]),
                    ("LineTo", vec![("X", 0.0), ("Y", 0.0)]),
                ],
            ),
            "#FF0000",
        )
    };
    // Unflipped: right angle at the lower left, so (1.3, 2.3) is inside and (2.5, 1.5) is outside (x + y > 4 is outside... 4.0 is the edge).
    let plain = draw(&one_page(vec![triangle("0", "0")]));
    assert!(plain.is_red(1.3, 2.3) && plain.is_red(1.5, 1.5));
    assert!(plain.is_white(2.7, 2.7));
    let mirrored_x = draw(&one_page(vec![triangle("1", "0")]));
    assert!(mirrored_x.is_red(2.7, 1.3), "FlipX puts the right angle at the lower right");
    assert!(mirrored_x.is_white(1.3, 2.3), "where the unflipped one was filled");
    let mirrored_y = draw(&one_page(vec![triangle("0", "1")]));
    assert!(mirrored_y.is_red(1.3, 2.7), "FlipY puts the right angle at the upper left");
    assert!(mirrored_y.is_white(1.3, 1.3 - 0.0) || mirrored_y.is_white(2.7, 1.3), "the lower part is empty");
}

#[test]
fn a_turn_is_counterclockwise_about_the_pin() {
    // A bar 2 inches long with its pin at its left end, pointing right; a quarter turn counterclockwise points it up. A clockwise turn
    // would point it down.
    let bar = |angle: f64| {
        filled(geometry_of(shape_at(1, 1.0, 2.0, 2.0, 0.5).cell("LocPinY", "0.25").cell("Angle", &angle.to_string()), rect_rows(2.0, 0.5)), "#FF0000")
    };
    let up = draw(&one_page(vec![bar(FRAC_PI_2)]));
    assert!(up.is_red(1.0, 3.0), "counterclockwise points it up");
    assert!(up.is_white(1.0, 1.0), "not down");
    let right = draw(&one_page(vec![bar(0.0)]));
    assert!(right.is_red(2.0, 2.0) && right.is_white(1.0, 3.0));
}

#[test]
fn a_page_scale_takes_drawing_units_to_paper_but_not_the_line_weight() {
    // DrawingScale 2 over PageScale 1: s = 0.5. An 8-unit page is 4 inches; a shape at (2, 2) of 4 by 4 units is at (1, 1) of 2 by 2 inches.
    let sb = filled(rect(1, 2.0, 2.0, 4.0, 4.0), "#FF0000").cell("LineWeight", "0.2").cell("LineColor", "#000000");
    let p = diacraft_testkit::build::PageBuilder::new()
        .cell("PageWidth", "8")
        .cell("PageHeight", "8")
        .cell("PageScale", "1")
        .cell("DrawingScale", "2")
        .shape(sb);
    let doc = base().page(1, p).build();
    let pic = draw(&doc);
    assert!(pic.is_red(2.0, 2.0));
    assert!(pic.is_white(3.5, 3.5), "paper is 4 inches, so 3.5 is past the 2 inch shape");
    assert!(pic.is_dark(0.93, 2.0), "the line is 0.2 inch on paper whatever the scale: 0.07 outside the edge is inside it");
    assert!(pic.is_dark(1.07, 2.0));
}

#[test]
fn a_background_page_is_drawn_behind_its_page() {
    let back = page(4.0, 4.0).background().shape(filled(rect(5, 0.5, 0.5, 3.0, 3.0), "#00FF00"));
    let front = page(4.0, 4.0).back_page(2).shape(filled(rect(6, 1.0, 1.0, 1.0, 1.0), "#FF0000"));
    let doc = base().page(1, front).page(2, back).build();
    let pic = draw(&doc);
    assert!(pic.is(0.7, 0.7, (0, 255, 0), 6), "the background's shape shows");
    assert!(pic.is_red(1.5, 1.5), "the page's own shape is in front of it");
}

#[test]
fn display_mode_puts_a_groups_own_drawing_behind_or_in_front_of_its_members() {
    let build = |mode: &str| {
        let child = filled(rect(2, 0.5, 0.5, 1.0, 1.0), "#FF0000");
        let group =
            filled(geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), rect_rows(2.0, 2.0)), "#0000FF").cell("DisplayMode", mode).group().child(child);
        draw(&one_page(vec![group]))
    };
    assert!(build("2").is(2.0, 2.0, (0, 0, 255), 6), "2: the group's own drawing is in front");
    assert!(build("1").is_red(2.0, 2.0), "1: behind");
    let hidden = build("0");
    assert!(hidden.is_red(2.0, 2.0) && hidden.is_white(1.2, 1.2), "0: the group's drawing is not drawn, and its member is");
}

#[test]
fn no_fill_pattern_and_no_show_and_no_fill_flags_leave_a_shape_unfilled() {
    let none = draw(&one_page(vec![filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000").cell("FillPattern", "0").cell("LineWeight", "0.1")]));
    assert!(none.is_white(2.0, 2.0));
    assert!(none.is_dark(1.0, 2.0), "the line stays");
    let no_fill = draw(&one_page(vec![geom_flag(filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000"), "NoFill", "1")]));
    assert!(no_fill.is_white(2.0, 2.0));
    let no_show = draw(&one_page(vec![geom_flag(filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000"), "NoShow", "1")]));
    assert_eq!(no_show.ink_pixels(), 0, "NoShow draws neither");
    let no_line = draw(&one_page(vec![geom_flag(filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000").cell("LineWeight", "0.1"), "NoLine", "1")]));
    assert!(no_line.is_red(1.0, 2.0), "without its line the fill reaches the edge unpainted over");
}

#[test]
fn the_themed_marker_is_asked_of_the_theme_and_not_read_as_zero() {
    // FillForegnd holds the marker and the formula THEMEVAL(): with no theme in the document the call names no property and gives the
    // root style's cell, here a blue-grey, where reading the marker as the number 0 would paint black and the
    // column of MS-VSDX's table would paint #96AFCF.
    let sb = rect(1, 1.0, 1.0, 2.0, 2.0).formula("FillForegnd", "THEMEVAL()", "Themed");
    let doc = base().root(|r| r.cell("FillForegnd", "#3C5A78")).page(1, page(4.0, 4.0).shape(sb)).build();
    let pic = draw(&doc);
    assert!(pic.is(2.0, 2.0, (0x3C, 0x5A, 0x78), 2), "got {:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_rounded_corner_leaves_the_corner_pixel_unfilled_and_the_edges_filled() {
    let sb = filled(rect(1, 1.0, 1.0, 2.0, 2.0), "#FF0000").cell("Rounding", "0.5").cell("LinePattern", "0");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_white(1.03, 1.03), "a radius of half an inch cuts the corner away");
    assert!(pic.is_red(1.5, 1.03) && pic.is_red(1.03, 1.5), "the edges away from the corners are filled");
    assert!(pic.is_red(2.0, 2.0));
}

#[test]
fn an_arc_bows_to_the_right_of_travel_for_a_positive_bow() {
    // From (0, 0) to (2, 0) with A = 0.5: travelling in +x, right is -y, so the middle of the arc is 0.5 below the chord. Drawn flipped,
    // the arc would bulge above it.
    let arc = |bow: f64| {
        geometry_of(
            shape_at(1, 1.0, 2.0, 2.0, 0.5).cell("LineWeight", "0.06").cell("FillPattern", "0"),
            vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("ArcTo", vec![("X", 2.0), ("Y", 0.0), ("A", bow)])],
        )
    };
    let pic = draw(&one_page(vec![arc(0.5)]));
    assert!(pic.is_dark(2.0, 1.5), "the middle is half an inch below the chord at y = 2");
    assert!(!pic.is_dark(2.0, 2.5), "and not above it");
    let negative = draw(&one_page(vec![arc(-0.5)]));
    assert!(negative.is_dark(2.0, 2.5) && !negative.is_dark(2.0, 1.5), "a negative bow is to the left of travel");
}

#[test]
fn a_marker_the_selected_theme_cannot_answer_reads_the_root_styles_cell() {
    // A shape on a theme part whose lists the reader could not read (here, a part with none). Its FillPattern holds the
    // marker (as a third of page shapes' do): read as 0 the shape would be empty, and the root's solid fill is what stands. A themed shadow
    // reads the root's none, and the report says the theme did not answer.
    let theme = diacraft_doc::Theme {
        part: std::sync::Arc::from("/visio/theme/theme1.xml"),
        bytes: std::sync::Arc::from(&b""[..]),
        colors: None,
        slots: Default::default(),
        major_font: None,
        minor_font: None,
        color_scheme_id: Some(diacraft_doc::SchemeId { scheme_enum: 1, scheme_guid: None }),
        font_scheme_id: Some(diacraft_doc::SchemeId { scheme_enum: 1, scheme_guid: None }),
        effect_scheme_id: Some(diacraft_doc::SchemeId { scheme_enum: 1, scheme_guid: None }),
        connector_scheme_id: Some(diacraft_doc::SchemeId { scheme_enum: 1, scheme_guid: None }),
        theme_scheme_id: Some(diacraft_doc::SchemeId { scheme_enum: 1, scheme_guid: None }),
        entries: Default::default(),
    };
    let sb = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("FillForegnd", "#FF0000")
        .cell("ColorSchemeIndex", "1")
        .cell("ThemeIndex", "1")
        .cell("EffectSchemeIndex", "1")
        .cell("ConnectorSchemeIndex", "1")
        .cell("FontSchemeIndex", "1")
        .cell("QuickStyleFillMatrix", "1")
        .cell("QuickStyleEffectsMatrix", "1")
        .formula("FillPattern", "THEMEVAL()", "Themed")
        .formula("ShdwPattern", "THEMEVAL()", "Themed");
    let mut doc = one_page(vec![sb]);
    doc.themes.push(std::sync::Arc::new(theme));
    let pic = draw(&doc);
    assert!(pic.is_red(2.0, 2.0), "got {:?}", pic.at(2.0, 2.0));
    assert!(pic.r.count(diacraft_render::SkipKind::ThemeUnanswered) >= 1, "{:?}", pic.r.skipped);
}
