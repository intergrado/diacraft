//! The craft-fonts build input, run as the build script runs it: `build_input.rs` is included here, so the code under test is the
//! file `build.rs` compiles. Nothing it does may fail a build.

#[path = "../../build_input.rs"]
mod under_test;

use std::path::{Path, PathBuf};

use diacraft_fonts::synth::SynthFont;
use under_test::{Row, build, checked_dir, is_excluded_family, parse_manifest, read_manifest, rerun_directives, resolve_file};

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Checkout {
        let dir = std::env::temp_dir().join(format!("diacraft-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("fonts")).expect("create checkout");
        Checkout(dir)
    }
    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, bytes).expect("write");
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn row(file: &str) -> Row {
    Row { family: "F".into(), style: "Regular".into(), file: file.into(), scripts: vec![] }
}

#[test]
fn a_manifest_line_gives_a_row() {
    let (rows, problems) =
        parse_manifest("# a comment\n\nInter | Regular | fonts/Inter.ttf | Latn,Cyrl | OFL-1.1 | OFL.txt | note | https://example.invalid\n");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        rows,
        vec![Row { family: "Inter".into(), style: "Regular".into(), file: "fonts/Inter.ttf".into(), scripts: vec!["Latn".into(), "Cyrl".into()] }]
    );
}

#[test]
fn malformed_lines_are_reported_and_skipped_without_losing_the_good_ones() {
    let (rows, problems) =
        parse_manifest("only two | fields\nGood | Regular | fonts/g.ttf | Latn\n | Regular | fonts/x.ttf | Latn\nNoFile | Regular |  | Latn\n");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].family, "Good");
    assert_eq!(problems.len(), 3, "{problems:?}");
}

#[test]
fn adobe_and_source_han_families_are_never_embedded() {
    assert!(is_excluded_family("Source Sans 3") && is_excluded_family("Source Han Serif") && is_excluded_family("Noto Sans CJK JP"));
    assert!(!is_excluded_family("Inter") && !is_excluded_family("Noto Sans"));
    let (rows, _) = parse_manifest("Source Sans 3 | Regular | fonts/a.ttf | Latn\nInter | Regular | fonts/b.ttf | Latn\n");
    assert_eq!(rows.iter().map(|r| r.family.as_str()).collect::<Vec<_>>(), vec!["Inter"]);
}

#[test]
fn a_manifest_is_cut_at_4096_rows_and_not_one_before() {
    let line = "Fam | Regular | fonts/f.ttf | Latn\n";
    for (count, rows, problems) in [(4095usize, 4095usize, 0usize), (4096, 4096, 0), (4097, 4096, 1), (5000, 4096, 1)] {
        let (r, p) = parse_manifest(&line.repeat(count));
        assert_eq!((r.len(), p.len()), (rows, problems), "{count} lines");
    }
}

#[test]
fn the_latin_manifest_is_read_before_the_main_one() {
    let c = Checkout::new("order");
    c.write("fonts/a.ttf", &SynthFont::new("A").ascii(500).build());
    c.write("fonts/manifest.txt", b"Main One | Regular | fonts/a.ttf | Latn\n");
    c.write("fonts/latin-manifest.txt", b"Latin One | Regular | fonts/a.ttf | Latn\n");
    let (src, warnings) = build(Some(&c.0), false);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (latin, main) = (src.find("Latin One").unwrap(), src.find("Main One").unwrap());
    assert!(latin < main, "the Latin faces come first");
}

#[test]
fn a_file_name_that_leaves_the_checkout_is_refused() {
    let c = Checkout::new("escape");
    c.write("fonts/real.ttf", &SynthFont::new("Real").ascii(500).build());
    c.write("outside.ttf", &SynthFont::new("Outside").ascii(500).build());
    let inside = c.0.join("sub");
    std::fs::create_dir_all(&inside).expect("sub");
    assert!(resolve_file(&c.0, &row("fonts/real.ttf")).is_ok());
    let up = "sub/../outside.ttf";
    assert!(resolve_file(&inside, &row("../outside.ttf")).unwrap_err().contains("outside"));
    assert!(resolve_file(&c.0, &row(up)).is_ok(), "a path that stays inside, however it is spelled");
    assert!(resolve_file(&c.0, &row("../../../../etc/passwd")).is_err());
    assert!(resolve_file(&c.0, &row("/etc/passwd")).is_err(), "an absolute name replaces the checkout path when joined, so it is outside");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_checkout_is_refused() {
    let c = Checkout::new("link");
    let outside = Checkout::new("link-target");
    outside.write("secret.ttf", &SynthFont::new("Secret").ascii(500).build());
    std::os::unix::fs::symlink(outside.0.join("secret.ttf"), c.0.join("fonts/link.ttf")).expect("symlink");
    assert!(resolve_file(&c.0, &row("fonts/link.ttf")).unwrap_err().contains("outside"));
}

#[test]
fn a_directory_and_a_tiny_file_are_not_fonts() {
    let c = Checkout::new("kinds");
    c.write("fonts/tiny.ttf", b"short");
    assert!(resolve_file(&c.0, &row("fonts")).is_err());
    assert!(resolve_file(&c.0, &row("fonts/tiny.ttf")).is_err());
    assert!(resolve_file(&c.0, &row("fonts/missing.ttf")).is_err());
}

#[test]
fn a_file_over_the_cap_is_refused_without_being_read() {
    let c = Checkout::new("big");
    let path = c.0.join("fonts/big.ttf");
    let f = std::fs::File::create(&path).expect("create");
    f.set_len(under_test::MAX_FILE_BYTES + 1).expect("sparse file");
    assert!(resolve_file(&c.0, &row("fonts/big.ttf")).is_err());
}

#[test]
fn unset_means_an_empty_list_and_no_warning() {
    let (src, warnings) = build(None, false);
    assert!(warnings.is_empty());
    assert_eq!(src, "pub(crate) static CRAFT_FONTS: &[CraftFont] = &[\n];\n");
}

#[test]
fn a_checkout_is_embedded_and_a_broken_one_is_only_warned_about() {
    let c = Checkout::new("embed");
    c.write("fonts/a.ttf", &SynthFont::new("A").ascii(500).build());
    c.write("fonts/manifest.txt", b"A | Regular | fonts/a.ttf | Latn\nMissing | Regular | fonts/missing.ttf | Latn\nbad line\n");
    let (src, warnings) = build(Some(&c.0), false);
    assert!(src.contains("family: \"A\"") && src.contains("include_bytes!"), "{src}");
    assert!(!src.contains("Missing"));
    // The missing latin manifest, the missing file and the bad line: three things to say, none of them an error.
    assert_eq!(warnings.len(), 3, "{warnings:?}");
}

#[test]
fn a_checkout_with_no_manifests_gives_an_empty_list() {
    let c = Checkout::new("empty");
    let (src, warnings) = build(Some(&c.0), false);
    assert!(!src.contains("include_bytes!"));
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let missing = Path::new("/nonexistent-craft-fonts-dir");
    let (src, warnings) = build(Some(missing), false);
    assert!(!src.contains("include_bytes!"));
    assert!(!warnings.is_empty());
}

#[test]
fn the_web_build_embeds_nothing() {
    let c = Checkout::new("wasm");
    c.write("fonts/a.ttf", &SynthFont::new("A").ascii(500).build());
    c.write("fonts/manifest.txt", b"A | Regular | fonts/a.ttf | Latn\n");
    let (src, warnings) = build(Some(&c.0), true);
    assert!(!src.contains("include_bytes!"));
    assert!(warnings.is_empty());
}

#[test]
fn a_family_name_with_quotes_and_newlines_cannot_break_the_generated_source() {
    let c = Checkout::new("quote");
    c.write("fonts/a.ttf", &SynthFont::new("A").ascii(500).build());
    c.write("fonts/manifest.txt", "Evil\"; panic!(); \" | Regular | fonts/a.ttf | Latn\n".as_bytes());
    let (src, _) = build(Some(&c.0), false);
    assert!(src.contains("Evil\\\"; panic!(); \\\""), "the name is written as an escaped literal: {src}");
}

#[test]
fn the_total_embedded_size_is_capped() {
    let c = Checkout::new("total");
    // Five sparse files of exactly the single-file cap: past the total cap (192 MiB, four of them) the fifth is left out.
    let mut manifest = String::new();
    for i in 0..5 {
        let path = c.0.join(format!("fonts/f{i}.ttf"));
        let mut f = std::fs::File::create(&path).expect("create");
        std::io::Write::write_all(&mut f, &[0, 1, 0, 0]).expect("a font's header");
        f.set_len(under_test::MAX_FILE_BYTES).expect("sparse");
        manifest.push_str(&format!("F{i} | Regular | fonts/f{i}.ttf | Latn\n"));
    }
    c.write("fonts/manifest.txt", manifest.as_bytes());
    let (src, warnings) = build(Some(&c.0), false);
    let embedded = src.matches("include_bytes!").count();
    assert_eq!(embedded, 4, "192 MiB holds four of 48 MiB, not five");
    assert!(warnings.iter().any(|w| w.contains("left out")), "{warnings:?}");
}

#[test]
fn a_file_that_is_not_a_font_is_never_embedded_whatever_the_manifest_says() {
    let c = Checkout::new("notfont");
    c.write("fonts/notes.ttf", b"this is a text file of more than twelve bytes");
    c.write("fonts/real.ttf", &SynthFont::new("Real").ascii(500).build());
    assert!(resolve_file(&c.0, &row("fonts/notes.ttf")).unwrap_err().contains("not a font"));
    assert!(resolve_file(&c.0, &row("fonts/real.ttf")).is_ok());
    for head in [&[0u8, 1, 0, 0][..], b"OTTO", b"true", b"ttcf"] {
        assert!(under_test::is_font_header(head), "{head:?}");
    }
    for head in [&b"\x7fELF"[..], b"OTT", b"", b"%PDF", b"PK\x03\x04", &[0, 1, 0, 1]] {
        assert!(!under_test::is_font_header(head), "{head:?}");
    }
}

#[test]
fn a_directory_that_could_end_up_in_a_directive_or_in_source_is_refused_and_the_build_goes_on() {
    use std::ffi::OsStr;
    let (dir, warnings) = checked_dir(Some(OsStr::new("/home/me/craft-fonts")));
    assert_eq!(dir, Some(PathBuf::from("/home/me/craft-fonts")));
    assert!(warnings.is_empty());
    assert_eq!(checked_dir(None), (None, vec![]));
    assert_eq!(checked_dir(Some(OsStr::new(""))), (None, vec![]));
    // A newline would end the line and let the rest be read as a directive: `cargo::error=BOOM` once failed the build this way.
    for bad in ["/x\ncargo::error=BOOM", "/x\rcargo::error=BOOM", "/x\u{7}", "/x\u{0}y"] {
        let (dir, warnings) = checked_dir(Some(OsStr::new(bad)));
        assert_eq!((dir, warnings.len()), (None, 1), "{bad:?}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let (dir, warnings) = checked_dir(Some(OsStr::from_bytes(b"/fonts/\xff\xfe")));
        assert_eq!((dir, warnings.len()), (None, 1), "a path that is not UTF-8 would not compile into include_bytes");
    }
    let lines = rerun_directives(Some(Path::new("/home/me/craft-fonts")));
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l.starts_with("cargo::rerun-if-") && !l.contains('\n')), "{lines:?}");
    assert_eq!(rerun_directives(None).len(), 2);
}

#[test]
fn a_manifest_that_is_a_pipe_or_too_big_is_refused_and_does_not_hang_the_build() {
    let c = Checkout::new("fifo");
    #[cfg(unix)]
    {
        let fifo = c.0.join("fonts/manifest.txt");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success());
        if made {
            assert!(read_manifest(&fifo).is_err());
            let (src, warnings) = build(Some(&c.0), false);
            assert!(!src.contains("include_bytes!"));
            assert!(warnings.iter().any(|w| w.contains("not a regular file")), "{warnings:?}");
            std::fs::remove_file(&fifo).expect("remove the pipe");
        } else {
            eprintln!("SKIPPED the pipe case: mkfifo is not available");
        }
    }
    c.write("fonts/big.txt", &vec![b'#'; (under_test::MAX_MANIFEST_BYTES + 1) as usize]);
    assert!(read_manifest(&c.0.join("fonts/big.txt")).is_err());
    c.write("fonts/small.txt", b"# a comment\n");
    assert_eq!(read_manifest(&c.0.join("fonts/small.txt")).unwrap(), "# a comment\n");
}

#[test]
fn a_file_of_exactly_48_mib_is_embedded_and_one_byte_more_is_not_and_eleven_bytes_is_not_a_font() {
    let c = Checkout::new("edges");
    let sparse = |name: &str, len: u64| {
        let path = c.0.join(format!("fonts/{name}"));
        let mut f = std::fs::File::create(&path).expect("create");
        std::io::Write::write_all(&mut f, &[0, 1, 0, 0]).expect("a font's header");
        f.set_len(len).expect("sparse");
    };
    sparse("at.ttf", under_test::MAX_FILE_BYTES);
    sparse("over.ttf", under_test::MAX_FILE_BYTES + 1);
    sparse("eleven.ttf", 11);
    sparse("twelve.ttf", 12);
    assert!(resolve_file(&c.0, &row("fonts/at.ttf")).is_ok());
    assert!(resolve_file(&c.0, &row("fonts/over.ttf")).is_err());
    assert!(resolve_file(&c.0, &row("fonts/eleven.ttf")).is_err(), "no font is eleven bytes");
    assert!(resolve_file(&c.0, &row("fonts/twelve.ttf")).is_ok());
}
