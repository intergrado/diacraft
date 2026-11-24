//! The assets the web build fetches: what the page's manifest says of them, read and judged without a browser.
//!
//! The data the module does not carry (the splash picture, the cover, the licence list: `diacraft_ui_egui::Asset`) is in hashed files of the app's site beside the app, and `manifest.json` at its root, which
//! `cargo xtask site --web` writes, names each file with its size and its SHA-256. The loader (`start.js`) reads the manifest and hands its text to the program; [`parse`] is the program's
//! reading of it. **A manifest is not trusted to be small or well formed**: its text is bounded, a name is a file of the site's `assets/` folder and nothing else, a size is at most the
//! window's limit for that asset ([`Asset::limit`]), and a digest is 64 hexadecimal digits. An entry that fails is that asset's failure in words, and the others are not touched.

use diacraft_ui_egui::Asset;
use serde_json::Value;

/// The most bytes of manifest the program reads. The file is under 2 KB.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// The folder of the site, beside the app, that holds the assets.
pub const ASSET_DIR: &str = "assets/";

/// What the manifest says of one asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The file, from the app's folder: `assets/name-hash.ext`.
    pub file: String,
    /// Its size in bytes: no more than [`Asset::limit`], and exactly what is read.
    pub bytes: usize,
    /// Its SHA-256.
    pub sha256: [u8; 32],
}

/// What each asset has in the manifest: its entry, or the words for why it has none.
pub type Table = [(Asset, Result<Entry, String>); 3];

/// Reads the `assets` of the manifest's text. Every asset has an answer: an entry, or why not (the manifest has none for it, or its entry is not what it must be). A text that is not a
/// manifest at all fails every asset with the same words.
pub fn parse(text: &str) -> Table {
    let whole = |why: String| Asset::ALL.map(|asset| (asset, Err(why.clone())));
    if text.len() > MAX_MANIFEST_BYTES {
        return whole(format!("the page's manifest is longer than {MAX_MANIFEST_BYTES} bytes"));
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(e) => return whole(format!("the page's manifest is not JSON ({e})")),
    };
    let Some(assets) = value.get("assets").and_then(Value::as_object) else {
        return whole("the page's manifest lists no assets".to_owned());
    };
    Asset::ALL.map(|asset| {
        let entry = match assets.get(asset.key()) {
            Some(entry) => entry_of(asset, entry),
            None => Err(format!("the page's manifest has no file for {}", asset.words())),
        };
        (asset, entry)
    })
}

fn entry_of(asset: Asset, value: &Value) -> Result<Entry, String> {
    let bad = |what: &str| format!("the page's manifest has {what} for {}", asset.words());
    let file = value.get("file").and_then(Value::as_str).ok_or_else(|| bad("no file name"))?;
    if !is_asset_file(file) {
        return Err(bad("a file name that is not a file of the site's assets folder"));
    }
    let bytes = value.get("bytes").and_then(Value::as_u64).ok_or_else(|| bad("no size"))?;
    let bytes =
        usize::try_from(bytes).ok().filter(|n| *n > 0 && *n <= asset.limit()).ok_or_else(|| bad("a size that is not one the window takes"))?;
    let digest = value.get("sha256").and_then(Value::as_str).ok_or_else(|| bad("no digest"))?;
    let sha256 = hex32(digest).ok_or_else(|| bad("a digest that is not 64 hexadecimal digits"))?;
    Ok(Entry { file: file.to_owned(), bytes, sha256 })
}

/// Whether `file` is a file of the site's assets folder, by a plain name: `assets/` and then letters, digits, `.`, `-` and `_`, no further folder, no name that starts with a dot, nothing that
/// could leave the folder or name another site.
pub fn is_asset_file(file: &str) -> bool {
    let Some(name) = file.strip_prefix(ASSET_DIR) else { return false };
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !name.contains("..")
}

/// 64 hexadecimal digits (either case) as 32 bytes.
pub fn hex32(text: &str) -> Option<[u8; 32]> {
    // Every byte a hexadecimal digit, first: `u8::from_str_radix` alone takes a leading `+`.
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let digits = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(digits, 16).ok()?;
    }
    Some(out)
}

/// The words for a download that came out the wrong length: the file the manifest named is not the file the site served.
pub fn wrong_length(asset: Asset, expected: usize, got: usize) -> String {
    let which = if got > expected { "longer" } else { "shorter" };
    format!("{} came {which} than the manifest says ({got} bytes, not {expected})", asset.words())
}

/// The words for a download whose digest is not the manifest's.
pub fn wrong_digest(asset: Asset) -> String {
    format!("{} does not match the digest in the manifest (the download is damaged)", asset.words())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"version":1,"assets":{
        "splash":{"file":"assets/diacraft-splash-0123456789abcdef.png","bytes":210952,"sha256":"0000000000000000000000000000000000000000000000000000000000000001"},
        "cover":{"file":"assets/diacraft-cover-0123456789abcdef.png","bytes":2403101,"sha256":"00000000000000000000000000000000000000000000000000000000000000ff"},
        "licenses":{"file":"assets/third-party-licenses-web-0123456789abcdef.txt","bytes":225292,"sha256":"ABCDEFabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123"}}}"#;

    fn entry(table: &Table, asset: Asset) -> &Result<Entry, String> {
        &table.iter().find(|(a, _)| *a == asset).expect("every asset has an answer").1
    }

    #[test]
    fn a_manifest_gives_every_asset_its_file_size_and_digest() {
        let table = parse(GOOD);
        let splash = entry(&table, Asset::Splash).as_ref().expect("an entry");
        assert_eq!(splash.file, "assets/diacraft-splash-0123456789abcdef.png");
        assert_eq!(splash.bytes, 210_952);
        assert_eq!(splash.sha256[31], 1);
        assert!(splash.sha256[..31].iter().all(|b| *b == 0));
        assert_eq!(entry(&table, Asset::Cover).as_ref().map(|e| e.bytes), Ok(2_403_101));
        let licences = entry(&table, Asset::Licenses).as_ref().expect("an entry");
        assert_eq!(licences.sha256[0], 0xab, "either case of hexadecimal");
    }

    #[test]
    fn what_is_not_a_manifest_fails_every_asset_in_words() {
        for text in ["", "not json", "[]", "{}", r#"{"assets":[]}"#, r#"{"assets":null}"#] {
            let table = parse(text);
            for (asset, answer) in &table {
                let why = answer.as_ref().expect_err("no entry from a text that is no manifest");
                assert!(why.contains("manifest"), "{asset:?}: {why}");
            }
        }
        let long = format!("{{\"assets\":{{}},\"pad\":\"{}\"}}", "x".repeat(MAX_MANIFEST_BYTES));
        assert!(parse(&long).iter().all(|(_, e)| e.as_ref().is_err_and(|w| w.contains("longer than"))));
    }

    #[test]
    fn an_asset_the_manifest_leaves_out_fails_alone() {
        let text = r#"{"assets":{"splash":{"file":"assets/s-0.png","bytes":5,"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}}"#;
        let table = parse(text);
        assert!(entry(&table, Asset::Splash).is_ok());
        for asset in [Asset::Cover, Asset::Licenses] {
            let why = entry(&table, asset).as_ref().expect_err("left out");
            assert!(why.contains("has no file for") && why.contains(asset.words()), "{why}");
        }
    }

    #[test]
    fn a_name_that_is_not_a_plain_file_of_the_assets_folder_is_refused() {
        for file in [
            "",
            "assets/",
            "assets",
            "/assets/a.png",
            "assets/../app.js",
            "assets/a/b.png",
            "assets/.hidden",
            "assets/a b.png",
            "assets/a?x=1.png",
            "assets/a#b.png",
            "assets\\a.png",
            "https://example.com/assets/a.png",
            "//example.com/assets/a.png",
            "assets/a..png",
            "other/a.png",
            "parts/a.wasm",
        ] {
            assert!(!is_asset_file(file), "{file:?}");
            let text = format!(r#"{{"assets":{{"splash":{{"file":{file:?},"bytes":5,"sha256":"{}"}}}}}}"#, "0".repeat(64));
            assert!(entry(&parse(&text), Asset::Splash).is_err(), "{file:?}");
        }
        assert!(is_asset_file("assets/diacraft-cover-0123456789abcdef.png"));
        assert!(!is_asset_file(&format!("assets/{}", "a".repeat(129))));
    }

    #[test]
    fn a_size_is_positive_and_within_the_limit_of_that_asset() {
        let with = |bytes: &str| format!(r#"{{"assets":{{"splash":{{"file":"assets/s.png","bytes":{bytes},"sha256":"{}"}}}}}}"#, "0".repeat(64));
        assert!(entry(&parse(&with("1")), Asset::Splash).is_ok());
        assert!(entry(&parse(&with(&Asset::Splash.limit().to_string())), Asset::Splash).is_ok());
        for bad in ["0", "-1", &(Asset::Splash.limit() + 1).to_string(), "1.5", "\"5\"", "null", "18446744073709551615", "1e3"] {
            assert!(entry(&parse(&with(bad)), Asset::Splash).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_digest_is_sixty_four_hexadecimal_digits_and_nothing_else() {
        assert_eq!(hex32(&"ab".repeat(32)), Some([0xab; 32]));
        assert_eq!(hex32(&"AB".repeat(32)), Some([0xab; 32]));
        for bad in [
            "",
            &"a".repeat(63),
            &"a".repeat(65),
            &"g".repeat(64),
            &format!("{}é", "a".repeat(62)),
            &format!("+{}", "a".repeat(63)),
            &format!(" {}", "a".repeat(63)),
        ] {
            assert_eq!(hex32(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_words_for_a_damaged_download_name_the_asset_and_the_difference() {
        assert_eq!(wrong_length(Asset::Cover, 10, 12), "the cover picture came longer than the manifest says (12 bytes, not 10)");
        assert!(wrong_length(Asset::Splash, 10, 3).contains("shorter"));
        assert!(wrong_digest(Asset::Licenses).contains("the list of third-party licences") && wrong_digest(Asset::Licenses).contains("damaged"));
    }
}
