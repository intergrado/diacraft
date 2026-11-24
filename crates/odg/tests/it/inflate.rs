//! The census's finding on file `515a9b4e`: a read inflated every part of the package, 40 MB of embedded fonts that
//! nothing reads among them. **A read for the drawing's reader inflated the structure, the four XML parts and the files its content names by an `href` attribute, and no other** until
//! The model now keeps every entry no field reads (`Document.parts`), so `read` inflates and keeps them, charged to the media budget before each is inflated, and a
//! damaged one is left out and said. The package's own `read_deferred`, which the tests of the package use, still inflates only what a reader reads, and the comparison, which
//! compares every part's bytes, still inflates all.

use diacraft_odg::package::Package;
use diacraft_odg::{Limits, diff_packages, read};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A drawing with three frames showing `Pictures/shown.png`, `./Pictures/with%20space.png` and `Pictures/a&#98;.png` (a name written with a
/// reference), and beside them: `Pictures/unused.png`, a font of `font` zero bytes nothing names, and a macro.
fn drawing(font: usize) -> Drawing {
    let mut d = Drawing::a4().page(concat!(
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/shown.png"/></draw:frame>"#,
        r#"<draw:frame svg:x="1cm" svg:y="4cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="./Pictures/with%20space.png"/></draw:frame>"#,
        r#"<draw:frame svg:x="1cm" svg:y="7cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/a&#98;.png"/></draw:frame>"#,
    ));
    for name in ["Pictures/shown.png", "Pictures/with space.png", "Pictures/ab.png", "Pictures/unused.png"] {
        d.files.push((name.to_owned(), [PNG.to_vec(), vec![7u8; 32]].concat()));
    }
    d.files.push(("Fonts/big.ttf".to_owned(), vec![0u8; font]));
    d.files.push(("Basic/Standard/Module1.xml".to_owned(), b"<script/>".to_vec()));
    d
}

/// What reading `bytes` for the drawing's reader inflated.
fn inflated(bytes: &[u8]) -> u64 {
    read(bytes, &Limits::default()).unwrap().1.work.package.inflated
}

#[test]
fn a_part_nothing_reads_is_kept_by_the_model_and_a_part_a_frame_names_is_a_picture() {
    let (small, big) = (drawing(10).bytes(), drawing(8 << 20).bytes());
    let (doc, _) = read(&big, &Limits::default()).unwrap();
    // The three pictures are the package's bytes, found by their hrefs: plain, percent-encoded with a leading `./`, and with a reference.
    let pictures: Vec<&str> = (0..3)
        .map(|k| {
            doc.pages
                .get(0)
                .and_then(|p| p.shapes.get(k))
                .and_then(|s| s.foreign.as_ref())
                .and_then(|f| f.picture.as_ref())
                .map(|m| m.part.as_ref())
                .unwrap()
        })
        .collect();
    assert_eq!(pictures, ["Pictures/shown.png", "Pictures/with space.png", "Pictures/ab.png"]);
    // The font of 8 MiB is inflated, to be kept: the read costs what the small drawing's does and the font's bytes more, which is what the eager read of the package inflates.
    assert_eq!(inflated(&big), inflated(&small) + (8 << 20) - 10, "the font is the model's now");
    let mut work = diacraft_container::Work::default();
    Package::read_counted(&big, &Limits::default(), &mut work).unwrap();
    assert_eq!(work.inflated, inflated(&big), "a read inflates every file of the package");
    // The entries nothing reads are the model's parts, in the order of the directory, each tagged with what it is for: the unused picture, the font and the macro. The pictures the
    // frames show are pictures and not parts, and `content.xml` and the rest are the model's own.
    let held: Vec<(&str, diacraft_doc::PartRole, usize)> = doc.parts.iter().map(|p| (p.name.as_ref(), p.role, p.bytes.len())).collect();
    assert_eq!(
        held,
        [
            ("Pictures/unused.png", diacraft_doc::PartRole::Other, 40),
            ("Fonts/big.ttf", diacraft_doc::PartRole::Font, 8 << 20),
            ("Basic/Standard/Module1.xml", diacraft_doc::PartRole::MacroProject, 9),
        ]
    );
    assert!(doc.parts.iter().all(|p| p.format == diacraft_doc::Format::Odf));
}

#[test]
fn what_nothing_names_is_listed_and_not_read_and_the_comparison_still_reads_it() {
    let bytes = drawing(1000).bytes();
    let mut work = diacraft_container::Work::default();
    let package = Package::read_deferred(&bytes, &Limits::default(), &mut work).unwrap();
    assert!(package.parts().any(|n| n == "Fonts/big.ttf") && package.parts().any(|n| n == "Pictures/unused.png"), "the files are listed");
    assert_eq!(package.part("Fonts/big.ttf"), None, "and not inflated");
    assert_eq!(package.part("Pictures/unused.png"), None);
    assert_eq!(package.part("Pictures/shown.png").map(<[u8]>::len), Some(8 + 32));
    assert!(package.part("content.xml").is_some() && package.part("META-INF/manifest.xml").is_some());
    assert!(package.has_macros(), "a macro is known by its name");
    assert!(work.scan_events > 0, "the two parts were scanned for the names they hold");
    // The comparison compares every part's bytes: one differing byte of a part nothing reads is found.
    let mut other = drawing(1000);
    other.files.iter_mut().find(|(n, _)| n == "Fonts/big.ttf").unwrap().1[500] = 1;
    let diff = diff_packages(&bytes, &other.bytes(), &Limits::default()).unwrap();
    assert_eq!(diff.bytes_only, ["Fonts/big.ttf"]);
}

/// A package whose stored `Pictures/x.png` has a byte flipped in its data, so that its checksum no longer matches: nothing names it when
/// `unnamed`, and a frame does when not.
fn damaged_picture(unnamed: bool) -> Vec<u8> {
    let page = if unnamed {
        String::new()
    } else {
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/x.png"/></draw:frame>"#.to_owned()
    };
    let mut d = Drawing::a4().page(&page);
    let marker = b"MARKERMARKERMARKER";
    d.files.push(("Pictures/x.png".to_owned(), [&PNG[..], marker].concat()));
    let mut bytes = package(DRAWING, d.files());
    let at = bytes.windows(marker.len()).position(|w| w == marker).unwrap();
    bytes[at] ^= 0xFF;
    bytes
}

#[test]
fn damage_where_nothing_reads_does_not_stop_a_read_and_damage_where_something_does_is_refused() {
    let unnamed = damaged_picture(true);
    assert!(read(&unnamed, &Limits::default()).is_ok(), "a damaged file nothing names is not read");
    assert_eq!(Package::read(&unnamed, &Limits::default()).unwrap_err().class(), "zip_corrupt", "the eager read inflates it");
    // The same damage in a picture a frame names is refused, with the same class.
    assert_eq!(read(&damaged_picture(false), &Limits::default()).unwrap_err().class(), "zip_corrupt");
}

#[test]
fn a_font_an_svg_font_face_uri_names_is_kept_as_a_part_beside_the_face_that_names_it() {
    // ODF names an embedded font by `svg:font-face-uri xlink:href` in `office:font-face-decls` (found by the review: the first version
    // inflated every file any `href` named, so a font so named, the case the census found, was inflated for nothing). The reader reads an `href` of a
    // `draw:image` only, and the model keeps the font as a part of the package and the face that names it as a face of the font table.
    let make = |named: bool, font: usize| {
        let mut d = Drawing::a4().page(r#"<draw:frame svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/shown.png"/></draw:frame>"#);
        if named {
            d.fonts = concat!(
                r#"<style:font-face style:name="Big" svg:font-family="Big"><svg:font-face-src>"#,
                r#"<svg:font-face-uri xlink:href="Fonts/big.ttf" xlink:type="simple"><svg:font-face-format svg:string="truetype"/></svg:font-face-uri>"#,
                r#"</svg:font-face-src></style:font-face>"#
            )
            .to_owned();
        }
        d.files.push(("Pictures/shown.png".to_owned(), [PNG.to_vec(), vec![7u8; 32]].concat()));
        d.files.push(("Fonts/big.ttf".to_owned(), vec![0u8; font]));
        d.bytes()
    };
    let (named, tiny) = (make(true, 8 << 20), make(true, 10));
    assert_eq!(inflated(&named), inflated(&tiny) + (8 << 20) - 10, "the font is the model's, and costs its bytes");
    let (doc, _) = read(&named, &Limits::default()).unwrap();
    assert_eq!(doc.parts.iter().map(|p| (p.name.as_ref(), p.role)).collect::<Vec<_>>(), [("Fonts/big.ttf", diacraft_doc::PartRole::Font)]);
    assert_eq!(doc.fonts.len(), 1, "the declaration of the face is one face, though both parts declare it");
    assert_eq!(doc.fonts.first().map(|f| f.name_u.as_ref()), Some("Big"));
    // The font is in the package and the picture is read: only what the frame shows is inflated.
    let mut work = diacraft_container::Work::default();
    let package = Package::read_deferred(&named, &Limits::default(), &mut work).unwrap();
    assert!(package.parts().any(|n| n == "Fonts/big.ttf"));
    assert_eq!(package.part("Fonts/big.ttf"), None);
    assert_eq!(package.part("Pictures/shown.png").map(<[u8]>::len), Some(8 + 32));
}
