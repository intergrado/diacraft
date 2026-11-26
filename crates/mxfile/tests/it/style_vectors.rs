//! The vectors of the style specification, each test named after the vector or the rule it holds the reader to.
//!
//! This file has one section for each task that tests what a style or a label says: the first is the (S-R1 to S-R9, S-R17 to S-R23, of the style
//! spec), as the module `style_strings`, and the second is the (S-R25 and S-R26 of it: the labels), as the module `label`. Every input is a string a test
//! writes by hand from the spec.

// =====: style strings =============================================================================================================================

mod style_strings {
    use std::sync::Arc;

    use diacraft_mxfile::style::keys::{def_ref, parse_decimal, parse_flag, parse_number, stencil_payload};
    use diacraft_mxfile::style::{
        CellKind, Colour, Corners, Direction, FontStyle, HAlign, Key, Margins, Paints, Parent, Radius, Spec, Style, StyleCache, VAlign, Value,
        font_style_bits,
    };
    use diacraft_mxfile::{Limits, Reading};

    use crate::limits;

    /// A read in progress with a cache of styles: what the model task will hold.
    struct Run {
        cache: StyleCache,
        reading: Reading<'static>,
    }

    impl Run {
        fn new() -> Run {
            Run { cache: StyleCache::new(), reading: Reading::new(limits()) }
        }

        fn shape(&mut self, text: &str) -> Arc<Style> {
            self.cache.resolve(Some(text), CellKind::Shape, &mut self.reading).unwrap()
        }

        fn connector(&mut self, text: &str) -> Arc<Style> {
            self.cache.resolve(Some(text), CellKind::Connector, &mut self.reading).unwrap()
        }

        /// What the report holds for `class`: its count and its cases.
        fn warning(&self, class: &str) -> (u64, Vec<String>) {
            self.reading.report.warnings.get(class).map_or((0, Vec::new()), |n| (n.count, n.cases.clone()))
        }

        fn finish(self) -> Reading<'static> {
            let Run { cache, mut reading } = self;
            cache.finish(&mut reading).unwrap();
            reading
        }
    }

    const BAD: &str = "drawio_style_bad_value";
    const IGNORED: &str = "drawio_style_keys_ignored";

    fn rgb(r: u8, g: u8, b: u8) -> Colour {
        Colour::rgb(r, g, b)
    }

    /// The style with the hexadecimal colour `hex` written in full.
    fn hex(text: &str) -> Colour {
        let digits = text.trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).unwrap();
        Colour { r: byte(0), g: byte(2), b: byte(4), a: if digits.len() == 8 { byte(6) } else { 255 } }
    }

    /// Every key a style sets, as `key=value` sorted by the key's name and joined by `;`, so that two styles are compared as one string.
    fn render(style: &Style) -> String {
        let mut lines: Vec<String> = style
            .entries()
            .map(|(key, value)| {
                let shown = match value {
                    Value::Num(n) => format!("{n}"),
                    Value::Flag(f) => (if f { "1" } else { "0" }).to_owned(),
                    Value::Text(t) => t.to_owned(),
                    Value::Colour(Spec::Colour(c)) => c.to_hex(),
                    Value::Colour(Spec::Default) => "default".to_owned(),
                    Value::Colour(other) => format!("{other:?}"),
                };
                format!("{}={shown}", key.name())
            })
            .collect();
        lines.sort();
        lines.join(";")
    }

    fn paints(style: &Style) -> Paints {
        style.paints(Parent::Layer)
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- the grammar

    #[test]
    fn v1_a_named_style_applies_where_it_stands_and_not_first_or_last() {
        let mut run = Run::new();
        // `text` comes after the red fill: its `fillColor=none` takes the fill away.
        let a = run.shape("fillColor=#ff0000;text");
        assert_eq!(paints(&a).fill, None, "the fill the text style removes");
        // `text` comes first: the red fill after it wins, and the style's own align and no line stay.
        let b = run.shape("text;fillColor=#ff0000");
        assert_eq!(paints(&b).fill, Some(rgb(255, 0, 0)));
        assert_eq!(b.align(), HAlign::Left);
        assert_eq!(b.vertical_align(), VAlign::Top);
        assert_eq!(paints(&b).stroke, None, "the text style has no line");
    }

    #[test]
    fn e1_a_repeated_key_is_the_last_and_a_named_style_does_not_come_last_whatever_its_place() {
        let mut run = Run::new();
        let a = run.shape("fillColor=#111111;fillColor=#222222");
        assert_eq!(paints(&a).fill, Some(rgb(0x22, 0x22, 0x22)));
        // The repeat comes after the named style, which gave a fill of its own: a reader that applies named styles last gets `#DAE8FC`.
        let b = run.shape("blue;fillColor=#abcdef");
        assert_eq!(paints(&b).fill, Some(rgb(0xab, 0xcd, 0xef)));
        assert_eq!(paints(&b).gradient, Some(rgb(0x7E, 0xA6, 0xE0)), "the rest of the named style is kept");
        // And before it, the named style's fill wins.
        let c = run.shape("fillColor=#abcdef;blue");
        assert_eq!(paints(&c).fill, Some(rgb(0xDA, 0xE8, 0xFC)));
        // A repeat after a named style that takes the key away puts it back.
        let d = run.shape("text;fillColor=#010203;text;fillColor=#040506");
        assert_eq!(paints(&d).fill, Some(rgb(4, 5, 6)));
    }

    #[test]
    fn v2_a_leading_semicolon_starts_from_nothing() {
        let mut run = Run::new();
        let a = run.shape(";fillColor=#00ff00");
        assert_eq!(paints(&a).fill, Some(rgb(0, 255, 0)));
        assert_eq!(a.font_size(), 11.0, "the built-in 11, not the stylesheet's 12");
        assert_eq!(paints(&a).stroke, None, "no stroke colour unset: no line");
        assert!(!a.is_set(Key::FontSize) && !a.is_set(Key::Shape));
        let b = run.shape("fillColor=#00ff00");
        assert_eq!(paints(&b).stroke, Some(Colour::BLACK), "a black line");
        assert_eq!(b.font_size(), 12.0);
        // An empty style, and none at all, are the defaults alone (S-R2).
        let empty = run.shape("");
        let none = run.cache.resolve(None, CellKind::Shape, &mut run.reading).unwrap();
        assert_eq!(render(&empty), render(&none));
        assert!(Arc::ptr_eq(&empty, &none), "no style and an empty one are one style");
        assert_eq!(empty.font_size(), 12.0);
        // Only a first character of `;` does it: a space before it and a `;` later do not.
        let c = run.shape(" ;fillColor=#00ff00");
        assert_eq!(c.font_size(), 12.0);
    }

    #[test]
    fn v3_a_value_keeps_every_equals_sign_after_the_first() {
        let mut run = Run::new();
        let style = run.shape("image=data:image/png,iVBORw0KGgo=;rounded=1");
        assert_eq!(style.text(Key::Image), Some("data:image/png,iVBORw0KGgo="));
        assert!(style.flag(Key::Rounded));
        // A padding that is two signs, and a value that is only signs.
        let b = run.shape("image=data:image/png,AAAA==;resIcon===;fontFamily=a=b=c");
        assert_eq!(b.text(Key::Image), Some("data:image/png,AAAA=="));
        assert_eq!(b.text(Key::ResIcon), Some("=="));
        assert_eq!(b.text(Key::FontFamily), Some("a=b=c"));
    }

    #[test]
    fn nothing_is_trimmed_and_case_counts() {
        let mut run = Run::new();
        let style = run.shape(";fillColor=#ff0000; fillColor=#00ff00;FillColor=#0000ff;fillcolor=#ffffff;strokeColor =#123456");
        assert_eq!(paints(&style).fill, Some(rgb(255, 0, 0)), "only `fillColor` is the key");
        assert!(!style.is_set(Key::StrokeColor));
        let ignored = run.finish().report.warnings.get(IGNORED).cloned().unwrap();
        assert_eq!(ignored.count, 4, "` fillColor`, `FillColor`, `fillcolor` and `strokeColor ` are four keys the importer does not read");
        // A value is not trimmed either: a space after the sign makes a number a bad value (a colour with a space round it too).
        let mut run = Run::new();
        let b = run.shape(";fillColor= #ff0000;strokeColor=#ff0000 ");
        assert_eq!(paints(&b).fill, Some(Colour::WHITE), "a bad colour is `default`");
        assert_eq!(paints(&b).stroke, Some(Colour::BLACK));
        assert_eq!(run.warning(BAD).0, 2);
    }

    #[test]
    fn e3_an_empty_value_is_no_colour_and_no_value_and_a_number_that_is_not_there() {
        let mut run = Run::new();
        let a = run.shape("fillColor=;image=;fontFamily=;align=;fontSize=");
        assert_eq!(paints(&a).fill, None, "no colour for a colour key, though the default style had `default`");
        assert!(!a.is_set(Key::Image) && !a.is_set(Key::FontFamily) && !a.is_set(Key::Align));
        assert_eq!(a.align(), HAlign::Center);
        assert_eq!(a.font_size(), 11.0, "the built-in value of a number that is not there");
        assert_eq!(run.warning(BAD), (1, vec!["fontSize".to_owned()]), "only the number is a bad value");
    }

    #[test]
    fn e4_a_bare_key_is_a_named_style_that_is_not_there_and_turns_nothing_on() {
        let mut run = Run::new();
        let style = run.shape(";rounded;html;shadow;dashed;fillColor;strokeColor;swimlane2");
        assert!(!style.flag(Key::Rounded) && !style.html() && !style.flag(Key::Shadow) && !style.flag(Key::Dashed));
        assert_eq!(render(&style), "", "no key set at all");
        // And `rounded` after the connector's default `rounded=1` leaves it on: it was never a flag.
        let c = run.connector("rounded");
        assert!(c.flag(Key::Rounded));
        let reading = run.finish();
        assert!(!reading.report.warnings.contains_key(IGNORED), "a token with no `=` is not a key, so it is not an ignored key");
    }

    #[test]
    fn e5_a_megabyte_of_semicolons_is_one_pass_of_empty_pieces() {
        let mut run = Run::new();
        let text = ";".repeat(1 << 20);
        let style = run.shape(&text);
        assert_eq!(render(&style), "", "a leading `;` starts from nothing, and every piece is empty");
        let work = run.reading.report.work;
        assert_eq!(work.style_pieces, (1 << 20) + 1, "a piece for each `;` and one after the last, none of them more than once");
        assert_eq!(work.style_bytes, 2 << 20, "hashed once, walked once");
        // Not led by one, the same run is the default style: nine pairs and the empty pieces.
        let mut run = Run::new();
        let style = run.shape(&format!("a{}", ";".repeat(1 << 20)));
        assert_eq!(style.font_size(), 12.0);
        assert_eq!(run.reading.report.work.style_pieces, 9 + (1 << 20) + 1);
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- none

    #[test]
    fn v4_none_takes_a_key_away_from_the_default_and_from_a_named_style() {
        let mut run = Run::new();
        let a = run.shape("strokeColor=none;dashed=1");
        assert_eq!(paints(&a).stroke, None, "no outline at all");
        assert!(a.flag(Key::Dashed));
        assert!(!a.is_set(Key::StrokeColor));
        let end = run.connector("endArrow=none");
        assert_eq!(end.text(Key::EndArrow), None, "no mark at the end despite the default `classic`");
        assert_eq!(run.connector("").text(Key::EndArrow), Some("classic"));
        // `shape=none` has no shape name; `edgeStyle=none` no style; a named style's own `none` takes the key from the default.
        let b = run.shape("shape=none;edgeStyle=none;arrow");
        assert_eq!(b.text(Key::Shape), Some("arrow"), "the named style gives the name back after the `none`");
        let c = run.shape("arrow;shape=none;edgeStyle=none");
        assert_eq!(c.text(Key::Shape), None);
        assert_eq!(c.text(Key::EdgeStyle), None);
        // A key with no value reads by the built-in value of S-R9: a number, a flag.
        let d = run.shape("strokeWidth=5;strokeWidth=none;fontSize=none;rounded=1;rounded=none;endFill=0;endFill=none");
        assert_eq!(d.stroke_width(), 1.0);
        assert_eq!(d.font_size(), 11.0, "the stylesheet's 12 is the default style's, and `none` took it away");
        assert!(!d.flag(Key::Rounded));
        assert!(d.flag(Key::EndFill), "the built-in value of a flag that is on unless a style turns it off");
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- numbers and flags

    #[test]
    fn v5_a_text_where_a_number_belongs_is_the_built_in_value_with_one_warning_naming_the_key() {
        let mut run = Run::new();
        let a = run.shape("strokeWidth=abc");
        assert_eq!(a.stroke_width(), 1.0, "a 1 px line");
        assert_eq!(run.warning(BAD), (1, vec!["strokeWidth".to_owned()]));
        let b = run.shape("strokeWidth=0x2");
        assert_eq!(b.stroke_width(), 1.0, "not 2");
        // One warning for each key name, however many styles and cells hold the bad value.
        let c = run.shape("strokeWidth=abc;fontSize=big");
        let again = run.shape("strokeWidth=abc;fontSize=big");
        assert!(Arc::ptr_eq(&c, &again));
        assert_eq!(run.warning(BAD), (2, vec!["strokeWidth".to_owned(), "fontSize".to_owned()]));
    }

    #[test]
    fn s_r5_a_number_is_a_finite_decimal_and_everything_else_is_text() {
        let big = 10_000_000.0;
        for (text, want) in [
            ("12", Some(12.0)),
            ("0", Some(0.0)),
            ("-0", Some(0.0)),
            ("1e3", Some(1000.0)),
            ("1E3", Some(1000.0)),
            ("1e+3", Some(1000.0)),
            ("1e-2", Some(0.01)),
            (" 2", Some(2.0)),
            ("2 ", Some(2.0)),
            ("\t2\n", Some(2.0)),
            ("+3", Some(3.0)),
            (".5", Some(0.5)),
            ("5.", Some(5.0)),
            ("+.5e1", Some(5.0)),
            ("-12.25", Some(-12.25)),
            ("0.000", Some(0.0)),
            ("007", Some(7.0)),
            ("10000000", Some(big)),
            ("-10000000", Some(-big)),
            ("1e7", Some(big)),
            ("0x2", None),
            ("0X2", None),
            ("0x", None),
            ("0b11", None),
            ("0o7", None),
            ("1e", None),
            ("1e+", None),
            ("e5", None),
            (".", None),
            ("+", None),
            ("-", None),
            ("--1", None),
            ("+-1", None),
            ("1.2.3", None),
            ("1,5", None),
            ("1 2", None),
            ("1_000", None),
            ("Infinity", None),
            ("-Infinity", None),
            ("NaN", None),
            ("inf", None),
            ("nan", None),
            ("1e999", None),
            ("-1e999", None),
            ("10000001", None),
            ("-10000001", None),
            ("1e8", None),
            ("1e308", None),
            ("99999999999999999999999999999999", None),
            ("\u{ff11}", None),
            ("\u{661}", None),
            ("", None),
            ("   ", None),
            ("abc", None),
            ("1px", None),
            ("12%", None),
            ("true", None),
        ] {
            assert_eq!(parse_number(text, big), want, "{text:?}");
        }
        // The bound is the caller's, and a decimal is a decimal without one.
        assert_eq!(parse_number("100", 50.0), None);
        assert_eq!(parse_decimal("1e300"), Some(1e300));
        assert_eq!(parse_decimal("1e999"), None);
        assert_eq!(parse_decimal(&"9".repeat(65)), None, "past the 64 bytes a number is read from");
        assert_eq!(parse_decimal(&format!("0.{}1", "0".repeat(70))), None);
        assert!(parse_decimal(&"1".repeat(64)).is_some());
        // `-0` is 0 in sign too.
        assert!(parse_decimal("-0").unwrap().is_sign_positive());
    }

    #[test]
    fn s_r5_a_number_is_a_number_through_a_style_and_a_text_is_a_bad_value() {
        let mut run = Run::new();
        for (text, want) in [
            ("0x2", None),
            ("1e3", Some(1000.0)),
            ("-0", Some(0.0)),
            (" 2", Some(2.0)),
            ("2", Some(2.0)),
            ("Infinity", None),
            ("NaN", None),
            ("1e999", None),
        ] {
            let style = run.shape(&format!(";rotation={text}"));
            assert_eq!(style.get(Key::Rotation), want.map(Value::Num), "{text:?}");
            assert_eq!(style.rotation(), want.unwrap_or(0.0), "{text:?}");
        }
        assert_eq!(run.warning(BAD), (1, vec!["rotation".to_owned()]));
    }

    #[test]
    fn s_r5_a_flag_is_on_for_a_number_but_zero_and_for_true_and_off_for_all_else() {
        for (text, on) in [
            ("1", true),
            ("2", true),
            ("-1", true),
            ("0.5", true),
            ("1e300", true),
            ("true", true),
            ("0", false),
            ("-0", false),
            ("0.0", false),
            ("0e5", false),
            ("false", false),
            ("yes", false),
            ("TRUE", false),
            ("True", false),
            ("true ", false),
            ("", false),
            ("1e999", false),
            ("0x1", false),
        ] {
            assert_eq!(parse_flag(text), on, "{text:?}");
        }
        let mut run = Run::new();
        for (text, on) in [("1", true), ("0", false), ("true", true), ("yes", false), ("", false), ("2", true)] {
            let style = run.shape(&format!(";rounded={text}"));
            assert_eq!(style.flag(Key::Rounded), on, "{text:?}");
        }
        assert_eq!(run.warning(BAD).0, 0, "a flag is never a bad value");
        // A flag that is on unless a style turns it off.
        let style = run.shape(";endFill=0;exitPerimeter=false;entryPerimeter=1");
        assert!(!style.flag(Key::EndFill) && !style.flag(Key::ExitPerimeter) && style.flag(Key::EntryPerimeter));
        assert!(run.shape(";").flag(Key::EndFill), "on unless set");
    }

    #[test]
    fn s_r5_a_word_that_is_none_of_the_key_s_is_a_bad_value_with_the_default() {
        let mut run = Run::new();
        let a = run.shape(
            ";align=left;verticalAlign=bottom;direction=north;gradientDirection=east;elbow=vertical;labelPosition=right;verticalLabelPosition=top",
        );
        assert_eq!(
            (a.align(), a.vertical_align(), a.direction(), a.gradient_direction(), a.label_position(), a.vertical_label_position()),
            (HAlign::Left, VAlign::Bottom, Direction::North, Direction::East, HAlign::Right, VAlign::Top)
        );
        assert_eq!(run.warning(BAD).0, 0);
        let b = run.shape(";align=justify;verticalAlign=Top;direction=up;gradientDirection=;elbow=diagonal");
        assert_eq!(
            (b.align(), b.vertical_align(), b.direction(), b.gradient_direction()),
            (HAlign::Center, VAlign::Middle, Direction::East, Direction::South)
        );
        assert!(!b.is_set(Key::Align) && !b.is_set(Key::Elbow));
        assert_eq!(run.warning(BAD).1, vec!["align", "verticalAlign", "direction", "elbow"], "an empty value is no value, and no bad one");
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- defaults

    #[test]
    fn s_r7_a_shape_and_a_connector_start_from_their_own_defaults() {
        let mut run = Run::new();
        let shape = run.shape("");
        assert_eq!(
            render(&shape),
            "align=center;fillColor=default;fontColor=default;fontFamily=Helvetica;fontSize=12;perimeter=rectanglePerimeter;shape=label;strokeColor=default;verticalAlign=middle"
        );
        let connector = run.connector("");
        assert_eq!(
            render(&connector),
            "align=center;endArrow=classic;fontColor=default;fontFamily=Helvetica;fontSize=11;labelBackgroundColor=default;rounded=1;shape=connector;strokeColor=default;verticalAlign=middle"
        );
        // Another kind with the same text is another style.
        assert!(!Arc::ptr_eq(&shape, &run.connector("")) && run.cache.len() == 2);
        assert_eq!(run.connector("fontSize=9").font_size(), 9.0);
    }

    #[test]
    fn s_r7_default_is_the_light_mode_colour_of_its_key() {
        let mut run = Run::new();
        let style = run.shape(
            ";fillColor=default;strokeColor=default;fontColor=default;gradientColor=default;labelBackgroundColor=default;swimlaneFillColor=default",
        );
        let p = paints(&style);
        assert_eq!(
            [p.fill, p.stroke, p.font, p.gradient, p.label_background, p.swimlane_fill],
            [
                Some(rgb(255, 255, 255)),
                Some(rgb(0, 0, 0)),
                Some(rgb(0, 0, 0)),
                Some(rgb(0, 0, 0)),
                Some(rgb(255, 255, 255)),
                Some(rgb(255, 255, 255))
            ]
        );
        // A plain shape: a white fill, a black line and black text.
        let plain = paints(&run.shape(""));
        assert_eq!((plain.fill, plain.stroke, plain.font), (Some(Colour::WHITE), Some(Colour::BLACK), Some(Colour::BLACK)));
        assert_eq!((plain.gradient, plain.label_background, plain.swimlane_fill), (None, None, None));
        // A connector has no fill, a black line and a white label background.
        let connector = paints(&run.connector(""));
        assert_eq!((connector.fill, connector.stroke, connector.label_background), (None, Some(Colour::BLACK), Some(Colour::WHITE)));
    }

    #[test]
    fn s_r9_the_built_in_values_of_a_key_no_style_sets() {
        let mut run = Run::new();
        let s = run.shape(";");
        assert_eq!((s.stroke_width(), s.opacity(), s.fill_opacity(), s.stroke_opacity(), s.text_opacity()), (1.0, 1.0, 1.0, 1.0, 1.0));
        assert_eq!(s.font_style(), FontStyle { bold: false, italic: false, underline: false, strike: false });
        assert_eq!(s.margins(), Margins { top: 2.0, right: 2.0, bottom: 2.0, left: 2.0 }, "spacing 2 and the four others 0");
        assert_eq!((s.rotation(), s.header_size(), s.marker_size(true), s.marker_size(false)), (0.0, 40.0, 6.0, 6.0));
        assert_eq!((s.num(Key::ImageWidth), s.num(Key::ImageHeight)), (Some(42.0), Some(42.0)));
        assert_eq!((s.num(Key::ArcSize), s.num(Key::Size), s.num(Key::ExitX), s.num(Key::EntryY)), (None, None, None, None));
        assert_eq!((s.num(Key::ExitDx), s.num(Key::EntryDy)), (Some(0.0), Some(0.0)));
        assert!(!s.html() && !s.flag(Key::Shadow) && !s.flag(Key::Rounded) && !s.wraps());
        assert!(s.flag(Key::Horizontal) && s.flag(Key::ImageAspect) && s.flag(Key::Movable) && s.flag(Key::Left));
        assert_eq!(
            (s.align(), s.vertical_align(), s.direction(), s.gradient_direction()),
            (HAlign::Center, VAlign::Middle, Direction::East, Direction::South)
        );
        assert_eq!(s.image_align(), HAlign::Left);
        // The shadow of S-R9.
        assert_eq!(diacraft_mxfile::style::SHADOW_COLOUR, Colour::BLACK);
        assert_eq!((diacraft_mxfile::style::SHADOW_OPACITY, diacraft_mxfile::style::SHADOW_OFFSET), (0.25, (2.0, 3.0)));
        // `startSize` is a header's 23 under the swimlane style and 40 without it, and a mark's 6 px on a connector.
        assert_eq!(run.shape("swimlane").header_size(), 23.0);
        assert_eq!(run.connector("startSize=10").marker_size(true), 10.0);
        // `html` is off unless set.
        assert!(run.shape("html=1").html());
    }

    #[test]
    fn s_r22_the_spacing_adds_to_each_side_and_the_numbers_are_held() {
        let mut run = Run::new();
        // V14 of the mapping spec: spacing 2, spacingLeft 4.
        let a = run.shape("fontSize=12;fontFamily=Helvetica;fontStyle=3;align=left;verticalAlign=top;spacing=2;spacingLeft=4");
        assert_eq!(a.margins(), Margins { top: 2.0, right: 2.0, bottom: 2.0, left: 6.0 });
        assert_eq!(a.font_style(), FontStyle { bold: true, italic: true, underline: false, strike: false });
        assert_eq!((a.align(), a.vertical_align()), (HAlign::Left, VAlign::Top));
        assert_eq!(a.text(Key::FontFamily), Some("Helvetica"));
        // A font size is held to 1 to 500 px; an opacity to 0 to 100.
        for (text, px) in [("0", 1.0), ("-5", 1.0), ("0.5", 1.0), ("12", 12.0), ("500", 500.0), ("5000", 500.0), ("1e7", 500.0)] {
            assert_eq!(run.shape(&format!("fontSize={text}")).font_size(), px, "{text}");
        }
        let o = run.shape("opacity=150;fillOpacity=-20;strokeOpacity=50;textOpacity=25.5");
        assert_eq!((o.opacity(), o.fill_opacity(), o.stroke_opacity(), o.text_opacity()), (1.0, 0.0, 0.5, 0.255));
        assert!(run.shape("whiteSpace=wrap").wraps());
        for no in ["whiteSpace=nowrap", "whiteSpace=Wrap", "whiteSpace=", "whiteSpace=wrap "] {
            assert!(!run.shape(no).wraps(), "{no}");
        }
    }

    #[test]
    fn s_r22_the_font_style_bits_are_one_two_four_and_eight_whatever_the_others_are() {
        for v in 0..4096u32 {
            assert_eq!(font_style_bits(v), (v % 2 == 1, v / 2 % 2 == 1, v / 4 % 2 == 1, v / 8 % 2 == 1), "{v}");
        }
        for v in [u32::MAX, u32::MAX - 1, 1 << 31, (1 << 31) | 9] {
            assert_eq!(font_style_bits(v), (v & 1 == 1, v & 2 == 2, v & 4 == 4, v & 8 == 8));
        }
        let mut run = Run::new();
        for (text, want) in [
            ("0", (false, false, false, false)),
            ("1", (true, false, false, false)),
            ("5", (true, false, true, false)),
            ("15", (true, true, true, true)),
            ("1.9", (true, false, false, false)),
            ("-1", (false, false, false, false)),
            ("16", (false, false, false, false)),
        ] {
            let s = run.shape(&format!("fontStyle={text}")).font_style();
            assert_eq!((s.bold, s.italic, s.underline, s.strike), want, "{text}");
        }
    }

    #[test]
    fn s_r19_the_arc_sizes_of_a_rounded_corner() {
        let mut run = Run::new();
        // V7 of the mapping spec: a rectangle 15 percent, an absolute 24 is 12 px, a polygon 10 px.
        let plain = run.shape("rounded=1");
        assert_eq!(plain.corner(Corners::Box), Some(Radius::Percent(15.0)));
        assert_eq!(plain.corner(Corners::Polygon), Some(Radius::Px(10.0)));
        assert_eq!(run.shape("rounded=1;absoluteArcSize=1;arcSize=24").corner(Corners::Box), Some(Radius::Px(12.0)));
        assert_eq!(run.shape("rounded=1;arcSize=30").corner(Corners::Box), Some(Radius::Percent(30.0)));
        assert_eq!(run.shape("rounded=1;arcSize=80").corner(Corners::Box), Some(Radius::Percent(50.0)), "at most 50");
        assert_eq!(run.shape("rounded=1;arcSize=-4").corner(Corners::Box), Some(Radius::Percent(0.0)));
        assert_eq!(run.shape("rounded=1;arcSize=30").corner(Corners::Polygon), Some(Radius::Px(15.0)));
        assert_eq!(run.shape("rounded=1;arcSize=-30").corner(Corners::Polygon), Some(Radius::Px(0.0)));
        assert_eq!(run.shape("rounded=1;absoluteArcSize=1").corner(Corners::Box), Some(Radius::Px(7.5)), "the default 15, halved");
        assert_eq!(run.shape("absoluteArcSize=1;arcSize=24").corner(Corners::Box), None, "no `rounded`, no corner");
        assert_eq!(run.shape("").corner(Corners::Polygon), None);
        // A connector rounds its bends by default, 10 px, and `rounded=0` stops it.
        assert_eq!(run.connector("").bend_radius(), Some(10.0));
        assert_eq!(run.connector("arcSize=30").bend_radius(), Some(15.0));
        assert_eq!(run.connector("rounded=0").bend_radius(), None);
        assert_eq!(run.shape("").bend_radius(), None);
    }

    #[test]
    fn s_r18_a_dash_pattern_is_a_few_lengths_or_none() {
        let mut run = Run::new();
        assert_eq!(run.shape("dashPattern=3 3").dash_pattern(), Some(vec![3.0, 3.0]));
        assert_eq!(run.shape("dashPattern=1 4").dash_pattern(), Some(vec![1.0, 4.0]));
        assert_eq!(run.shape("dashPattern=1  4   1 4").dash_pattern(), Some(vec![1.0, 4.0, 1.0, 4.0]));
        assert_eq!(run.shape("dashPattern=8").dash_pattern(), Some(vec![8.0]));
        for none in [
            "",
            "dashPattern=",
            "dashPattern=a b",
            "dashPattern=1 -4",
            "dashPattern=1 NaN",
            "dashPattern=1e999 1",
            &format!("dashPattern={}", "1 ".repeat(17)),
        ] {
            assert_eq!(run.shape(none).dash_pattern(), None, "{none:?}");
        }
        assert_eq!(run.shape(&format!("dashPattern={}", "1 ".repeat(16))).dash_pattern().map(|d| d.len()), Some(16));
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- named styles

    /// What each named style leaves in a style that starts from nothing (a leading `;`), written from the table of S-R8: a key that a `none` takes away is not there.
    const NAMED: &[(&str, &str)] = &[
        ("text", "align=left;verticalAlign=top"),
        ("edgeLabel", "align=left;fontSize=11;labelBackgroundColor=default;verticalAlign=top"),
        ("label", "align=left;fontStyle=1;imageHeight=42;imageWidth=42;rounded=1;spacing=2;spacingLeft=52;verticalAlign=middle"),
        (
            "icon",
            "align=center;fontStyle=0;imageAlign=center;imageHeight=48;imageWidth=48;labelBackgroundColor=default;rounded=1;spacing=0;spacingLeft=0;spacingTop=6;verticalAlign=top;verticalLabelPosition=bottom",
        ),
        ("swimlane", "fontSize=12;fontStyle=1;shape=swimlane;startSize=23"),
        ("group", "verticalAlign=top"),
        ("ellipse", "perimeter=ellipsePerimeter;shape=ellipse"),
        ("rhombus", "perimeter=rhombusPerimeter;shape=rhombus"),
        ("triangle", "perimeter=trianglePerimeter;shape=triangle"),
        ("line", "labelBackgroundColor=default;shape=line;spacingTop=8;strokeWidth=4;verticalAlign=top"),
        ("image", "labelBackgroundColor=default;shape=image;verticalAlign=top;verticalLabelPosition=bottom"),
        ("roundImage", "labelBackgroundColor=default;perimeter=ellipsePerimeter;shape=image;verticalAlign=top;verticalLabelPosition=bottom"),
        ("rhombusImage", "labelBackgroundColor=default;perimeter=rhombusPerimeter;shape=image;verticalAlign=top;verticalLabelPosition=bottom"),
        ("arrow", "fillColor=default;shape=arrow"),
        ("fancy", "glass=1;shadow=1"),
    ];

    /// The fill, the gradient and the stroke of the nine colours.
    const COLOURS: &[(&str, &str, &str, &str)] = &[
        ("gray", "#F5F5F5", "#B3B3B3", "#666666"),
        ("blue", "#DAE8FC", "#7EA6E0", "#6C8EBF"),
        ("green", "#D5E8D4", "#97D077", "#82B366"),
        ("turquoise", "#D5E8D4", "#67AB9F", "#6A9153"),
        ("yellow", "#FFF2CC", "#FFD966", "#D6B656"),
        ("orange", "#FFCD28", "#FFA500", "#D79B00"),
        ("red", "#F8CECC", "#EA6B66", "#B85450"),
        ("pink", "#E6D0DE", "#B5739D", "#996185"),
        ("purple", "#E1D5E7", "#8C6C9C", "#9673A6"),
    ];

    #[test]
    fn s_r8_every_named_style_adds_what_the_table_says() {
        let mut run = Run::new();
        for (name, want) in NAMED {
            assert_eq!(render(&run.shape(&format!(";{name}"))), *want, "{name}");
        }
        for (name, fill, gradient, stroke) in COLOURS {
            let colours = format!("fillColor={fill};gradientColor={gradient};strokeColor={stroke}");
            let mut want: Vec<String> = colours.split(';').map(str::to_owned).chain(["glass=1".to_owned(), "shadow=1".to_owned()]).collect();
            want.sort();
            assert_eq!(render(&run.shape(&format!(";{name}"))), want.join(";"), "{name}");
            let mut plain: Vec<String> = colours.split(';').map(str::to_owned).collect();
            plain.sort();
            assert_eq!(render(&run.shape(&format!(";plain-{name}"))), plain.join(";"), "plain-{name}");
        }
    }

    #[test]
    fn v6_a_colour_style_has_its_three_colours_and_a_shadow_and_a_gloss_and_the_plain_one_has_none() {
        let mut run = Run::new();
        let blue = run.shape("blue");
        let p = paints(&blue);
        assert_eq!((p.fill, p.gradient, p.stroke), (Some(rgb(0xDA, 0xE8, 0xFC)), Some(rgb(0x7E, 0xA6, 0xE0)), Some(rgb(0x6C, 0x8E, 0xBF))));
        assert!(blue.flag(Key::Shadow) && blue.flag(Key::Glass));
        let plain = run.shape("plain-blue");
        assert_eq!((paints(&plain).fill, paints(&plain).gradient, paints(&plain).stroke), (p.fill, p.gradient, p.stroke));
        assert!(!plain.flag(Key::Shadow) && !plain.flag(Key::Glass));
        // A named style that is not in the stylesheet is ignored, and so are a CSS declaration and a bare colour code (the census's two slips).
        let nothing = run.shape(";nosuchstyle;color:red;#ff0000;Blue");
        assert_eq!(render(&nothing), "");
    }

    #[test]
    fn s_r8_a_named_style_takes_the_default_style_s_keys_away_and_applies_on_a_connector() {
        let mut run = Run::new();
        let text = run.shape("text");
        assert_eq!((paints(&text).fill, paints(&text).stroke, paints(&text).gradient), (None, None, None));
        assert_eq!(paints(&text).font, Some(Colour::BLACK), "the default's font colour stays");
        assert_eq!(text.text(Key::Shape), Some("label"));
        let label = run.connector("edgeLabel");
        assert_eq!(label.font_size(), 11.0);
        assert_eq!(paints(&label).label_background, Some(Colour::WHITE));
        assert_eq!(paints(&label).stroke, None, "the text style it extends has no line");
        // `group` on a shape: invisible.
        let group = run.shape("group");
        assert_eq!((paints(&group).fill, paints(&group).stroke), (None, None));
        assert_eq!(group.vertical_align(), VAlign::Top);
    }

    #[test]
    fn s_r8_the_named_styles_are_sorted_each_pair_is_a_good_value_and_an_extended_style_extends_none() {
        use diacraft_mxfile::style::named::{NAMED, find};
        assert!(NAMED.windows(2).all(|w| w[0].name < w[1].name), "sorted, for the binary search");
        for named in NAMED {
            assert_eq!(find(named.name).map(|n| n.name), Some(named.name));
            if let Some(base) = named.extends.and_then(find) {
                assert!(base.extends.is_none(), "{} extends {} that extends", named.name, base.name);
            } else {
                assert!(named.extends.is_none(), "{} extends a style that is not there", named.name);
            }
            let mut run = Run::new();
            for (key, value) in named.pairs.iter().chain(named.extends.and_then(find).map(|b| b.pairs).unwrap_or(&[])) {
                let key = Key::from_name(key).unwrap_or_else(|| panic!("{}: {key} is not a key the importer reads", named.name));
                // The value reads as its key's kind.
                let style = run.shape(&format!(";{}={value}", key.name()));
                assert!(*value == "none" || style.is_set(key), "{}: {key}={value} is not a good value", named.name);
            }
            assert_eq!(run.warning(BAD).0, 0, "{}", named.name);
            assert_eq!(named.len(), named.pairs.len() + named.extends.and_then(find).map_or(0, |b| b.pairs.len()));
        }
        assert!(find("Text").is_none() && find("").is_none() && find("plain").is_none());
        assert_eq!(NAMED.len(), 15 + 9 + 9, "fifteen named, nine colours and their nine plain twins");
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- colours

    /// The colour a fill of `text` gives on a style that starts from nothing, with its warning count.
    fn fill_of(text: &str) -> (Option<Spec>, u64) {
        let mut run = Run::new();
        let style = run.shape(&format!(";fillColor={text}"));
        let spec = match style.get(Key::FillColor) {
            Some(Value::Colour(spec)) => Some(spec),
            Some(other) => panic!("{text:?} gave {other:?}"),
            None => None,
        };
        (spec, run.warning(BAD).0)
    }

    #[test]
    fn s_r17_every_form_of_a_colour() {
        let c = |r, g, b, a| Some(Spec::Colour(Colour { r, g, b, a }));
        for (text, want) in [
            ("#f00", c(255, 0, 0, 255)),
            ("#F00", c(255, 0, 0, 255)),
            ("#ff0000", c(255, 0, 0, 255)),
            ("#FF0000", c(255, 0, 0, 255)),
            ("#123456", c(0x12, 0x34, 0x56, 255)),
            ("#fff", c(255, 255, 255, 255)),
            ("#abc", c(0xaa, 0xbb, 0xcc, 255)),
            ("#ff000080", c(255, 0, 0, 0x80)),
            ("#f008", c(255, 0, 0, 0x88)),
            ("rgb(255, 0, 0)", c(255, 0, 0, 255)),
            ("rgb(255,0,0)", c(255, 0, 0, 255)),
            ("RGB(255 0 0)", c(255, 0, 0, 255)),
            ("rgb( 1 , 2 , 3 )", c(1, 2, 3, 255)),
            ("rgb(100%, 0%, 50%)", c(255, 0, 128, 255)),
            ("rgb(300, -5, 0)", c(255, 0, 0, 255)),
            ("rgb(1.4, 1.6, 0.5)", c(1, 2, 1, 255)),
            ("rgba(255, 0, 0, 0.5)", c(255, 0, 0, 128)),
            ("rgba(255, 0, 0, 50%)", c(255, 0, 0, 128)),
            ("rgb(255 0 0 / 50%)", c(255, 0, 0, 128)),
            ("rgba(0,0,0,2)", c(0, 0, 0, 255)),
            ("rgba(0,0,0,-1)", c(0, 0, 0, 0)),
            ("hsl(0, 100%, 50%)", c(255, 0, 0, 255)),
            ("hsl(120, 100%, 50%)", c(0, 255, 0, 255)),
            ("hsl(240deg 100% 50%)", c(0, 0, 255, 255)),
            ("hsl(0.5turn, 100%, 50%)", c(0, 255, 255, 255)),
            ("hsl(-120, 100%, 50%)", c(0, 0, 255, 255)),
            ("hsl(480, 100%, 50%)", c(0, 255, 0, 255)),
            ("hsl(0, 0%, 100%)", c(255, 255, 255, 255)),
            ("hsl(0, 0%, 0%)", c(0, 0, 0, 255)),
            ("hsla(0, 100%, 50%, 0.25)", c(255, 0, 0, 64)),
            ("red", c(255, 0, 0, 255)),
            ("Red", c(255, 0, 0, 255)),
            ("RED", c(255, 0, 0, 255)),
            ("cornflowerblue", c(100, 149, 237, 255)),
            ("rebeccapurple", c(102, 51, 153, 255)),
            ("transparent", c(0, 0, 0, 0)),
            ("light-dark(#7EA6E0,#FF0000)", c(0x7E, 0xA6, 0xE0, 255)),
            ("light-dark(rgb(1, 2, 3), rgb(4, 5, 6))", c(1, 2, 3, 255)),
            ("light-dark( #fff , #000 )", c(255, 255, 255, 255)),
            ("LIGHT-DARK(red,blue)", c(255, 0, 0, 255)),
            ("light-dark(default,#000)", Some(Spec::Default)),
            ("light-dark(inherit,#000)", Some(Spec::Inherit)),
            ("default", Some(Spec::Default)),
            ("inherit", Some(Spec::Inherit)),
            ("fillColor", Some(Spec::Fill)),
            ("strokeColor", Some(Spec::Stroke)),
            ("parentFillColor", Some(Spec::ParentFill)),
            ("parentStrokeColor", Some(Spec::ParentStroke)),
        ] {
            assert_eq!(fill_of(text), (want, 0), "{text:?}");
        }
        // The two forms the spec's V8 names.
        assert_eq!(fill_of("light-dark(#7EA6E0,#FF0000)").0, Some(Spec::Colour(hex("#7EA6E0"))));
    }

    #[test]
    fn s_r17_any_other_text_is_the_default_with_one_warning() {
        for text in [
            "banana",
            "#ff",
            "#fffff",
            "#ggg",
            "#ggggggg",
            "#1234567",
            "#123456789",
            "#",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "rgb(a,b,c)",
            "rgb(1,2,3",
            "rgb 1,2,3)",
            "rgb (1,2,3)",
            "hsl(1,2)",
            "hsl(a, 1%, 2%)",
            "hsl(1rad, 1%, 2%)",
            "light-dark(red)",
            "light-dark()",
            "light-dark(light-dark(red,blue),blue)",
            "light-dark(banana,red)",
            " red",
            "red ",
            "Default",
            "INHERIT",
            "fillcolor",
            "currentcolor",
            "var(--a)",
            "\u{ff03}ff0000",
            &"#".repeat(300),
            &format!("rgb({})", " ".repeat(300)),
            &format!("{}red", " ".repeat(300)),
            &"light-dark(".repeat(100),
        ] {
            assert_eq!(fill_of(text), (Some(Spec::Default), 1), "{text:?}");
        }
        // An empty value is no colour and no warning; `none` takes the key away.
        assert_eq!(fill_of(""), (None, 0));
        assert_eq!(fill_of("none"), (None, 0));
        // The key name is what is warned about, once, for the fill and the line both.
        let mut run = Run::new();
        run.shape(";fillColor=x;strokeColor=y;fillColor=z");
        assert_eq!(run.warning(BAD), (2, vec!["fillColor".to_owned(), "strokeColor".to_owned()]));
    }

    #[test]
    fn v7_a_child_of_a_container_takes_the_parent_s_colours_and_one_on_a_layer_takes_default() {
        let mut run = Run::new();
        let container = run.shape("fillColor=#123456");
        let parent = paints(&container);
        let child = run.shape("strokeColor=parentFillColor;fillColor=inherit");
        let p = child.paints(Parent::Cell(&parent));
        assert_eq!((p.fill, p.stroke), (Some(rgb(0x12, 0x34, 0x56)), Some(rgb(0x12, 0x34, 0x56))), "the line and the fill both #123456");
        // The same child on a layer: `default` of each key, a black line and a white fill.
        let on_layer = child.paints(Parent::Layer);
        assert_eq!((on_layer.fill, on_layer.stroke), (Some(Colour::WHITE), Some(Colour::BLACK)));
        // `parentStrokeColor` is the parent's line, `inherit` is the parent's colour for the same key, whichever key.
        let outline = run.shape("strokeColor=#aabbcc;fontColor=#010203");
        let po = paints(&outline);
        let c2 = run.shape("fillColor=parentStrokeColor;strokeColor=inherit;fontColor=inherit;labelBackgroundColor=parentFillColor");
        let p2 = c2.paints(Parent::Cell(&po));
        assert_eq!((p2.fill, p2.stroke, p2.font), (Some(rgb(0xaa, 0xbb, 0xcc)), Some(rgb(0xaa, 0xbb, 0xcc)), Some(rgb(1, 2, 3))));
        assert_eq!(p2.label_background, Some(Colour::WHITE), "the parent's fill is the default's white");
        // The parent has none of it: the child has none.
        let bare = Paints::default();
        let p3 = c2.paints(Parent::Cell(&bare));
        assert_eq!((p3.fill, p3.stroke, p3.font, p3.label_background), (None, None, None, None));
        // A chain: each level resolved against the one above, so that `inherit` goes down as far as the cells do.
        let top = paints(&run.shape(";fillColor=#c0ffee"));
        let middle = run.shape(";fillColor=inherit").paints(Parent::Cell(&top));
        let bottom = run.shape(";fillColor=inherit").paints(Parent::Cell(&middle));
        assert_eq!(bottom.fill, Some(rgb(0xc0, 0xff, 0xee)));
    }

    #[test]
    fn s_r17_a_keyword_that_names_the_cell_s_own_colour_is_one_level_and_never_a_loop() {
        let mut run = Run::new();
        let style = |run: &mut Run, text: &str| paints(&run.shape(text));
        let a = style(&mut run, ";fillColor=#102030;strokeColor=fillColor;fontColor=fillColor;labelBackgroundColor=strokeColor");
        assert_eq!((a.stroke, a.font), (Some(rgb(0x10, 0x20, 0x30)), Some(rgb(0x10, 0x20, 0x30))));
        assert_eq!(a.label_background, Some(Colour::WHITE), "the line is itself a keyword: one level, so `default` of the label background");
        // The two name each other: neither is read from the other.
        let b = style(&mut run, ";fillColor=strokeColor;strokeColor=fillColor");
        assert_eq!((b.fill, b.stroke), (Some(Colour::WHITE), Some(Colour::BLACK)));
        // One names itself.
        let c = style(&mut run, ";fillColor=fillColor;strokeColor=strokeColor");
        assert_eq!((c.fill, c.stroke), (Some(Colour::WHITE), Some(Colour::BLACK)));
        // The cell's own fill is none: nothing to name.
        let d = style(&mut run, ";strokeColor=fillColor");
        assert_eq!(d.stroke, None);
        // A line of `strokeColor=#f00;fillColor=strokeColor` is fine in either order.
        let e = style(&mut run, ";fillColor=strokeColor;strokeColor=#ff0000");
        assert_eq!((e.fill, e.stroke), (Some(rgb(255, 0, 0)), Some(rgb(255, 0, 0))));
    }

    #[test]
    fn v8_light_dark_takes_its_first_colour_and_a_function_is_read() {
        assert_eq!(fill_of("light-dark(#7EA6E0,#FF0000)").0, Some(Spec::Colour(rgb(0x7E, 0xA6, 0xE0))));
        assert_eq!(fill_of("rgb(255, 0, 0)").0, Some(Spec::Colour(rgb(255, 0, 0))));
        let mut run = Run::new();
        let style = run.shape(";fillColor=light-dark(#7EA6E0,#FF0000);strokeColor=rgb(255, 0, 0)");
        assert_eq!((paints(&style).fill, paints(&style).stroke), (Some(rgb(0x7E, 0xA6, 0xE0)), Some(rgb(255, 0, 0))));
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- the keys

    #[test]
    fn the_keys_are_named_once_and_the_table_of_the_mapping_spec_is_all_there() {
        let mut names: Vec<&str> = Key::ALL.iter().map(|k| k.name()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "a name twice");
        for key in Key::ALL {
            assert_eq!(Key::from_name(key.name()), Some(*key));
            assert_eq!(key.to_string(), key.name());
        }
        assert_eq!(Key::COUNT, Key::ALL.len());
        // M-STYLE-TABLE, key by key.
        let table = "html rounded fillColor strokeColor whiteSpace align fontSize fontColor fontStyle fontFamily verticalAlign shape strokeWidth edgeStyle points endArrow startArrow endFill startFill \
                     endSize startSize entryX entryY entryDx entryDy exitX exitY exitDx exitDy entryPerimeter exitPerimeter overflow spacing spacingTop spacingBottom spacingLeft spacingRight \
                     left right top bottom labelBackgroundColor dashed dashPattern shadow horizontal verticalLabelPosition labelPosition arcSize absoluteArcSize container gradientColor \
                     gradientDirection curved image imageAspect imageWidth imageHeight imageAlign imageVerticalAlign size fixedSize rotation flipH flipV direction movable resizable rotatable \
                     deletable editable locked opacity fillOpacity strokeOpacity textOpacity swimlaneFillColor swimlaneLine elbow noLabel jumpStyle perimeter margin inheritText \
                     defaultLabelBackgroundColor portConstraint sourcePortConstraint targetPortConstraint connectable";
        let table: Vec<&str> = table.split_whitespace().collect();
        assert_eq!(table.len(), 89, "the table names eighty-nine keys, though its text says eighty-five");
        for name in &table {
            assert!(Key::from_name(name).is_some(), "{name} is a key the mapping spec reads");
        }
        // Beyond the table: the effects the model task counts, and the stencil icons that M-KEEP keeps.
        let extra: Vec<&str> = names.iter().copied().filter(|n| !table.contains(n)).collect();
        assert_eq!(extra, ["comic", "glass", "grIcon", "prIcon", "resIcon", "rough", "sketch"]);
        assert_eq!(Key::ALL.len(), 89 + 7);
        for name in ["", "Html", "fillcolor", "fillColor ", "pointerEvents", "jettySize", "libavoidRouting", "childLayout", "fontSource"] {
            assert!(Key::from_name(name).is_none(), "{name:?}");
        }
        assert!(Key::ALL.iter().filter(|k| k.is_lost()).map(|k| k.name()).eq(["glass", "sketch", "comic", "rough"]));
    }

    #[test]
    fn a_key_the_importer_does_not_read_changes_nothing() {
        let mut run = Run::new();
        let with = run.shape("rounded=1;pointerEvents=0;childLayout=stackLayout;fontSource=http://example.com/x;fillColor=#010203");
        let without = run.shape("rounded=1;fillColor=#010203");
        assert_eq!(render(&with), render(&without));
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- ignored keys

    #[test]
    fn s_r6_the_keys_the_importer_does_not_read_are_counted_by_name_once_for_each_cell() {
        let mut run = Run::new();
        for _ in 0..10 {
            run.shape("foo=1;bar=2;foo=3;rounded=1");
        }
        for _ in 0..5 {
            run.shape("baz=1");
        }
        run.connector("foo=9");
        let before = run.reading.report.work.style_pieces;
        let reading = run.finish();
        let noted = reading.report.warnings.get(IGNORED).unwrap();
        // foo twice in ten cells, once in a connector; bar ten; baz five. Known keys are not counted.
        assert_eq!(noted.count, 20 + 1 + 10 + 5);
        assert_eq!(noted.cases, ["foo 21", "bar 10", "baz 5"], "by how many times, then by name");
        // The finish visits the names each style kept, once for each style and not once for each cell: foo and bar, baz, foo of the connector's.
        assert_eq!(reading.report.work.style_pieces - before, 4);
    }

    #[test]
    fn s_r6_the_report_names_the_twenty_most_frequent_and_counts_the_rest() {
        let mut run = Run::new();
        // Twenty-five names; `k00` once, `k01` twice and so on, one cell for each, so that the most frequent are the last.
        for i in 0..25 {
            run.shape(&vec![format!("k{i:02}=1"); i + 1].join(";"));
        }
        let reading = run.finish();
        let noted = reading.report.warnings.get(IGNORED).unwrap();
        assert_eq!(noted.count, (1..=25u64).sum::<u64>(), "every use is counted, named or not");
        assert_eq!(noted.cases.len(), 20);
        assert_eq!(noted.cases.first().map(String::as_str), Some("k24 25"));
        assert_eq!(noted.cases.last().map(String::as_str), Some("k05 6"));
    }

    #[test]
    fn s_r6_an_effect_that_is_read_and_not_drawn_is_counted_as_ignored_when_the_file_writes_it() {
        let mut run = Run::new();
        let style = run.shape("sketch=1;glass=1;comic=1;rough=1;rounded=1");
        assert!(style.flag(Key::Sketch) && style.flag(Key::Glass) && style.flag(Key::Comic) && style.flag(Key::Rough));
        run.shape("blue"); // `fancy` adds `glass`, which the file did not write
        let reading = run.finish();
        let noted = reading.report.warnings.get(IGNORED).unwrap();
        assert_eq!(
            (noted.count, noted.cases.clone()),
            (4, vec!["comic 1".to_owned(), "glass 1".to_owned(), "rough 1".to_owned(), "sketch 1".to_owned()])
        );
    }

    #[test]
    fn s_r6_a_name_is_escaped_and_cut_in_the_report() {
        let mut run = Run::new();
        run.shape(&format!("{}=1;a\u{1b}[31mb=2", "x".repeat(500)));
        let reading = run.finish();
        let noted = reading.report.warnings.get(IGNORED).unwrap();
        assert_eq!(noted.count, 2);
        assert!(noted.cases.iter().all(|c| c.chars().count() <= 130), "{noted:?}");
        assert!(noted.cases.iter().any(|c| c.contains("\\u{1b}")), "a terminal escape is written out: {noted:?}");
        assert!(noted.cases.iter().all(|c| !c.contains('\u{1b}')));
    }

    #[test]
    fn s_r6_nothing_ignored_is_no_warning() {
        let mut run = Run::new();
        run.shape("rounded=1;html=1");
        assert!(!run.finish().report.warnings.contains_key(IGNORED));
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- the cache and its work

    #[test]
    fn a_style_text_is_parsed_once_for_every_cell_that_has_it() {
        let mut run = Run::new();
        let first = run.shape("a=1;b=2");
        let work = run.reading.report.work;
        assert_eq!((work.style_pieces, work.style_bytes), (9 + 2, 2 * 7), "nine defaults and two pieces; the bytes hashed, then walked");
        for _ in 0..1000 {
            let again = run.shape("a=1;b=2");
            assert!(Arc::ptr_eq(&first, &again));
        }
        let work = run.reading.report.work;
        assert_eq!(work.style_pieces, 9 + 2, "no piece twice");
        assert_eq!(work.style_bytes, 2 * 7 + 1000 * 7, "the lookup hashes the text again");
        assert_eq!(run.cache.len(), 1);
    }

    #[test]
    fn a_named_style_is_charged_for_the_keys_it_adds() {
        let mut run = Run::new();
        // Leading `;`: no defaults. Pieces: the empty one, `blue` (1 piece, and 3 + 2 keys: its own colours and `fancy`'s two), `text` (1 and 5).
        run.shape(";blue;text");
        assert_eq!(run.reading.report.work.style_pieces, 3 + 5 + 5);
        // `edgeLabel` is `text`'s five and two of its own.
        let mut run = Run::new();
        run.shape(";edgeLabel");
        assert_eq!(run.reading.report.work.style_pieces, 2 + 7);
        // A name that is not there costs the piece.
        let mut run = Run::new();
        run.shape(";nothing");
        assert_eq!(run.reading.report.work.style_pieces, 2);
    }

    #[test]
    fn a_style_past_the_budget_is_refused_and_not_kept() {
        let limits = Limits { max_read_work: 40, ..Limits::default() };
        let mut reading = Reading::new(&limits);
        let mut cache = StyleCache::new();
        // A small style reads: 3 bytes hashed and 3 walked, nine defaults and one piece.
        assert!(cache.resolve(Some("a=1"), CellKind::Shape, &mut reading).is_ok());
        assert_eq!(reading.report.work.total(), 3 + 3 + 9 + 1);
        let big = vec!["a=1"; 100].join(";");
        let refused = cache.resolve(Some(&big), CellKind::Shape, &mut reading).unwrap_err();
        assert_eq!(refused.class, "drawio_too_complex");
        assert!(refused.words.contains("more than 40 steps"), "{}", refused.words);
        assert_eq!(cache.len(), 1, "nothing kept for a style that was refused");
        assert!(reading.report.work.total() <= 40, "a charge that is refused is not counted");
        // A budget that has refused stays refused, as every other charge of the read does (`Cap`): not even the style the cache has is given out.
        assert!(cache.resolve(Some("b=1"), CellKind::Connector, &mut reading).is_err());
        assert!(cache.resolve(Some("a=1"), CellKind::Shape, &mut reading).is_err());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn the_two_kinds_of_one_text_are_two_styles_and_the_style_keeps_its_text() {
        let mut run = Run::new();
        let a = run.shape("image=data:image/png,AAAA;shape=image");
        let b = run.connector("image=data:image/png,AAAA;shape=image");
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(a.text(Key::Image), b.text(Key::Image));
        assert_eq!(run.cache.len(), 2);
        // The entries are in the order of the keys, and `get` says what each is.
        let keys: Vec<Key> = a.entries().map(|(k, _)| k).collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(a.get(Key::Image), Some(Value::Text("data:image/png,AAAA")));
        assert_eq!(a.get(Key::FontSize), Some(Value::Num(12.0)));
        assert_eq!(a.get(Key::Html), None);
        // A key that holds text is not a number and the other way round.
        assert_eq!((a.num(Key::Image), a.text(Key::FontSize)), (None, None));
    }

    // ---------------------------------------------------------------------------------------------------------------------------------------- names for text

    #[test]
    fn s_r3_a_def_reference_is_digits_between_def_and_a_bracket_and_the_style_is_not_expanded() {
        assert_eq!(def_ref("def(0)"), Some(0));
        assert_eq!(def_ref("def(12)"), Some(12));
        assert_eq!(def_ref("def(007)"), Some(7));
        for no in [
            "",
            "def()",
            "def(",
            "def)",
            "def(1",
            "def(a)",
            "def(-1)",
            "def(+1)",
            "def(1 )",
            "def( 1)",
            "Def(1)",
            "def(1)x",
            "xdef(1)",
            "def(1.5)",
            "def(\u{661})",
            "def(99999999999999999999)",
        ] {
            assert_eq!(def_ref(no), None, "{no:?}");
        }
        assert_eq!(def_ref(&format!("def({})", "9".repeat(18))), Some(999_999_999_999_999_999));
        let mut run = Run::new();
        let style = run.shape("image=def(3);shape=def(0)");
        assert_eq!(style.text(Key::Image), Some("def(3)"));
        assert_eq!(style.text(Key::Shape).and_then(def_ref), Some(0));
    }

    #[test]
    fn s_r14_a_stencil_is_the_text_between_stencil_and_the_last_character() {
        assert_eq!(stencil_payload("stencil(AbC+/=)"), Some("AbC+/="));
        assert_eq!(stencil_payload("stencil()"), Some(""));
        assert_eq!(stencil_payload("stencil(x"), Some(""), "the last character is taken without a check that it is a bracket");
        assert_eq!(stencil_payload("stencil(xyz]"), Some("xyz"));
        assert_eq!(stencil_payload("stencil(\u{e9}\u{e9}"), Some("\u{e9}"));
        for no in ["", "stencil", "stencil(", "Stencil(x)", " stencil(x)", "mxgraph.flowchart.card", "stencil[x]"] {
            assert_eq!(stencil_payload(no), None, "{no:?}");
        }
        let mut run = Run::new();
        let style = run.shape("shape=stencil(AAAA==)");
        assert_eq!(style.text(Key::Shape).and_then(stencil_payload), Some("AAAA=="));
    }
}

// =====: labels ====================================================================================================================================

/// S-R25 and S-R26. The cell's own style (`html=1`, `fontSize`) is the next stage's: a label here is read with the defaults of a cell.
mod label {
    use diacraft_container::xml::parse;
    use diacraft_mxfile::html::{LINE_BREAK, Link, Rgb, Run, RunFmt, read_plain, warn};
    use diacraft_mxfile::{Limits, Reading};

    use crate::label_table::{bold, count, fmt, html, italic, lines, plain, runs};

    #[test]
    fn v10_a_label_of_tags_a_script_and_a_picture_keeps_its_text_and_formats_and_counts_the_picture() {
        let got = html("<b>Title</b><br>Line &amp; <i>two</i><script>x()</script><img src=\"data:image/png,AAAA\">");
        // The text "Title", a line break, "Line & two".
        assert_eq!(lines(&got), [format!("Title{LINE_BREAK}Line & two")]);
        assert_eq!(
            runs(&got),
            [("Title".to_owned(), bold()), (format!("{LINE_BREAK}Line & "), fmt()), ("two".to_owned(), italic())],
            "\"Title\" bold, \"two\" italic, the rest as the cell says"
        );
        // Nothing of the script or the picture, and the count of the one that is counted: the picture.
        assert!(!lines(&got)[0].contains("x()") && !lines(&got)[0].contains("AAAA"));
        assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 1);
        assert_eq!(got.report.warnings.len(), 1, "and the script's dropping is not said");
    }

    #[test]
    fn v11_two_blocks_are_two_paragraphs_a_list_is_bulleted_and_a_font_has_its_html_size_and_colour() {
        let got = html("<div>A</div><div>B</div>");
        assert_eq!(lines(&got), ["A", "B"], "two paragraphs (the cell's `align=center` is the text's, the next stage's)");
        assert_eq!(got.label.paras.iter().map(|p| p.align).collect::<Vec<_>>(), [None, None]);
        let got = html("<ul><li>x</li><li>y</li></ul>");
        assert_eq!(lines(&got), ["\u{2022} x", "\u{2022} y"], "\"• x\" and \"• y\" as two paragraphs");
        let got = html("<font color=\"#ff0000\" size=\"5\">big</font>");
        let want = RunFmt { color: Some(Rgb::new(255, 0, 0)), size_px: Some(24.0), ..fmt() };
        assert_eq!(runs(&got), [("big".to_owned(), want)], "a red run at HTML size 5, 24 px (scaled by M-UNIT in the model)");
    }

    #[test]
    fn v12_a_span_style_gives_size_and_colour_and_not_position_and_a_javascript_link_gives_text_and_no_link() {
        let got = html("<span style=\"font-size:20px;color:#00f;position:fixed\">t</span>");
        let want = RunFmt { size_px: Some(20.0), color: Some(Rgb::new(0, 0, 255)), ..fmt() };
        assert_eq!(got.label.paras.len(), 1);
        assert_eq!(got.label.paras[0].runs, [Run { text: "t".to_owned(), fmt: want }], "a 20 px blue run and nothing for `position`");
        let got = html("<a href=\"javascript:alert(1)\">go</a>");
        assert_eq!(lines(&got), ["go"], "the text \"go\"");
        assert_eq!(got.label.link, None, "and no hyperlink");
        assert_eq!(
            html("<a href=\"https://example.com\">go</a>").label.link,
            Some(Link::Url("https://example.com".to_owned())),
            "while an http link is one"
        );
    }

    #[test]
    fn v13_a_plain_label_is_its_text_and_an_escaped_line_feed_is_a_second_line() {
        // `a<b>c` written as an attribute value, `a&lt;b&gt;c`, is read by the XML parser as `a<b>c`, which a plain label keeps as it is.
        let cell = parse(b"<mxCell value=\"a&lt;b&gt;c\"/>").unwrap();
        let value = cell.attr("value").unwrap();
        assert_eq!(value, "a<b>c");
        let got = plain(value);
        assert_eq!(lines(&got), ["a<b>c"], "the literal text `a<b>c`");
        assert_eq!(got.report.warnings.len(), 0);
        assert_eq!(got.label.paras[0].runs[0].fmt, fmt());
        // `one&#xa;two` is one, a line feed, two.
        let cell = parse(b"<mxCell value=\"one&#xa;two\"/>").unwrap();
        let value = cell.attr("value").unwrap();
        assert_eq!(value, "one\ntwo");
        let got = plain(value);
        assert_eq!(lines(&got), [format!("one{LINE_BREAK}two")], "two lines");
        assert_eq!(got.label.paras.len(), 1, "in one paragraph: a line break, not a new block");
        // In an HTML label the same text is white space, and `br` breaks the line (S-R26).
        assert_eq!(lines(&html("one\ntwo")), ["one two"]);
        assert_eq!(lines(&html("one<br>two")), [format!("one{LINE_BREAK}two")]);
        // And `\n` as two characters is two characters, in a plain label.
        assert_eq!(lines(&plain("one\\ntwo")), ["one\\ntwo"]);
        // The plain reader takes the text the same whatever limits it is held to that do not touch it.
        let limits = Limits::default();
        let mut reading = Reading::new(&limits);
        let again = read_plain(value, &Default::default(), &mut reading).unwrap();
        assert_eq!(again, got.label);
    }
}
