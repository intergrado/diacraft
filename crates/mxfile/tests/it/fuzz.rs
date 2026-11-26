//! The law of the fuzz target `mxfile_decode` (`fuzz/src/lib.rs`, included below), held to a seed corpus and to random bytes by the reader's own tests, so that every input the
//! target ever crashed on is replayed here through the very same code (add it to `CRASHES`, with the commit that fixed it), and so that the law is checked on every run of the
//! suite and not only by the fuzzer.

use proptest::prelude::*;

#[path = "../../fuzz/src/lib.rs"]
pub(crate) mod law;

/// Inputs the target crashed on, each with what it was.
const CRASHES: &[(&str, &[u8])] = &[];

/// The seeds: one of each mode, the spec's own vectors among them, and what a hostile file does.
fn seeds() -> Vec<Vec<u8>> {
    let with = |mode: u8, text: &str| -> Vec<u8> {
        let mut v = vec![mode];
        v.extend(text.bytes());
        v
    };
    let v1 = "UzV2zq1wL0osyPDNT0nNUTV2VTV2LsrPL4GwciucU3NyVI0MMlNUjV1UjYwMgFjVyA2HrCFY1qAgsSg1rwSLBiADYTaQg2Y1AA==";
    let v5 = "eNpTNXbOrXAvSizI8M1PSc1RNXZVNXYuys8vgbByK5xTc3JUjQwyU1SNXVSNjAyAWNXIDYesIVjWoCCxKDWvBIsGIANhNpCDZjUA9xsm+w==";
    vec![
        with(0, v1),
        with(0, v5),
        with(0, "UzV2zq1wL0osyPDNT0nNUTV2VY2KAgA="),
        with(0, "***"),
        with(0, ""),
        with(0, "  A   A  "),
        with(1, "<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel>"),
        with(1, "Caf\u{e9} 50% \u{FFFE}\u{1}\u{1F600}"),
        with(1, ""),
        with(2, "%3C%2F%C3%A9%ZZ%C3%28%E4%B8"),
        with(2, "ab=c=="),
        with(2, "\u{FFFE}\u{FFFF}\u{1}\t\n\r"),
        with(3, "<mxfile><diagram name=\"a\"/><diagram/></mxfile>"),
        with(
            3,
            "<mxfile><diagram>UzV2zq1wL0osyPDNT0nNUTV2VTV2LsrPL4GwciucU3NyVI0MMlNUjV1UjYwMgFjVyA2HrCFY1qAgsSg1rwSLBiADYTaQg2Y1AA==</diagram></mxfile>",
        ),
        with(3, "<!DOCTYPE mxfile [<!ENTITY a \"b\">]><mxfile/>"),
        with(3, "<mxGraphModel><root><mxCell id=\"0\"/></root></mxGraphModel>"),
        with(3, "<mxlibrary>[]</mxlibrary>"),
        with(3, "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><mxfile/>"),
        with(4, "AAAA"),
        with(4, "<mxGraphModel/>"),
        with(4, "\u{1}\u{2}&<>\""),
        vec![],
        vec![255; 64],
        vec![3; 1000],
    ]
}

#[test]
fn the_seeds_hold_the_law() {
    for seed in seeds() {
        law::check(&seed);
    }
}

#[test]
fn every_input_that_crashed_the_target_holds_the_law() {
    for (what, input) in CRASHES {
        eprintln!("{what}");
        law::check(input);
    }
}

#[test]
fn every_byte_of_every_mode_and_a_sweep_of_short_inputs_hold_the_law() {
    for mode in 0..=255u8 {
        law::check(&[mode]);
    }
    for a in (0..=255u8).step_by(5) {
        for b in (0..=255u8).step_by(7) {
            for mode in 0..5u8 {
                law::check(&[mode, a, b, b'<', a, b'=', b'%', b]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Any bytes, in any mode.
    #[test]
    fn any_bytes_hold_the_law(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        law::check(&data);
    }

    /// Text that looks like what the reader meets: Base64, percent sequences, tags and quotes, in every mode.
    #[test]
    fn text_of_the_kind_the_reader_meets_holds_the_law(mode in 0u8..5, text in "[A-Za-z0-9+/=%<>\"' \n\u{FFFE}\u{e9}-]{0,600}") {
        let mut data = vec![mode];
        data.extend(text.bytes());
        law::check(&data);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ `mxfile_read`

/// Inputs the target `mxfile_read` crashed on, each with what it was.
const READ_CRASHES: &[(&str, &[u8])] = &[];

/// The seeds of `mxfile_read`: the spec's own vectors for the cells, and what a hostile page does with ids, parents and numbers.
fn read_seeds() -> Vec<Vec<u8>> {
    let with = |mode: u8, text: &str| -> Vec<u8> {
        let mut v = vec![mode];
        v.extend(text.bytes());
        v
    };
    vec![
        with(
            0,
            "<mxfile><diagram name=\"a\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel></diagram></mxfile>",
        ),
        with(
            1,
            "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"2\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"1\" width=\"5\" as=\"geometry\"/></mxCell>",
        ),
        with(1, "<mxCell id=\"5\" vertex=\"1\" parent=\"9\"/><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>"),
        with(
            1,
            "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"3\" vertex=\"1\" parent=\"4\"/><mxCell id=\"4\" vertex=\"1\" parent=\"3\"/>",
        ),
        with(1, "<mxCell id=\"2\" vertex=\"1\"/><mxCell id=\"2\" vertex=\"1\"/><mxCell id=\"2~2\"/><mxCell id=\"2\"/>"),
        with(1, "<mxCell id=\"a\" parent=\"a\"/><object id=\"b\"><mxCell parent=\"b\"/></object><UserObject/>"),
        with(
            1,
            "<mxCell id=\"0\"><mxGeometry x=\"nan\" y=\"1e999\" width=\"-1\" as=\"geometry\"><Array as=\"points\"><mxPoint x=\"1e30\"/></Array></mxGeometry></mxCell>",
        ),
        with(0, "<mxfile vars=\"{&quot;a&quot;:[[[[1]]]],&quot;b&quot;:&quot;\\ud83d\\ude00&quot;}\"><diagram/></mxfile>"),
        with(0, "<mxfile vars=\"{&quot;a&quot;:\"><diagram/></mxfile>"),
        with(
            0,
            "<mxGraphModel backgroundImage=\"{&quot;src&quot;:&quot;data:page/id,x&quot;}\" pageWidth=\"1e9\"><defs><def data=\"x\"/></defs><root/></mxGraphModel>",
        ),
        with(2, "\u{0}\u{1}\u{2}\u{3}\u{4}\u{5}\u{6}\u{7}\u{8}\u{9}\u{a}\u{b}"),
        with(3, "\u{3}\u{0}\u{1}\u{9}\u{2}\u{4}\u{9}\u{1}\u{3}\u{1}\u{9}\u{5}"),
        vec![2, 7, 0, 1, 0, 3, 3, 9, 3, 2, 8, 3, 4, 4, 27, 9, 5, 6, 31, 11],
        vec![],
        vec![255; 64],
        vec![2; 1000],
    ]
}

#[test]
fn the_read_seeds_hold_the_law() {
    for seed in read_seeds() {
        law::read::check(&seed);
    }
}

#[test]
fn every_input_that_crashed_mxfile_read_holds_the_law() {
    for (what, input) in READ_CRASHES {
        eprintln!("{what}");
        law::read::check(input);
    }
}

#[test]
fn every_byte_of_every_read_mode_and_a_sweep_of_recipes_hold_the_law() {
    for mode in 0..=255u8 {
        law::read::check(&[mode]);
    }
    for a in (0..=255u8).step_by(11) {
        for b in (0..=255u8).step_by(13) {
            for mode in 0..4u8 {
                law::read::check(&[mode, a, b, a ^ b, b.wrapping_add(a), a, 9, b, 3, a.wrapping_mul(3), b, 17, a, b ^ 0x55, 4, 9]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Any bytes, in any mode.
    #[test]
    fn any_bytes_hold_the_read_law(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        law::read::check(&data);
    }

    /// A recipe of cells that collide: ids and parents from a handful, flags and numbers from the bytes.
    #[test]
    fn a_recipe_of_colliding_cells_holds_the_read_law(mode in 2u8..4, data in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut input = vec![mode];
        input.extend(data);
        law::read::check(&input);
    }

    /// Text of the kind the reader of cells meets inside a root.
    #[test]
    fn text_of_the_kind_a_root_holds_holds_the_read_law(text in "[A-Za-z0-9<>/=\" .-]{0,500}") {
        let mut input = vec![1u8];
        input.extend(text.bytes());
        law::read::check(&input);
    }
}
