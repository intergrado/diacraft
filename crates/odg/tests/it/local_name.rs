//! An archive whose local header names an entry other than its central row does. The `odg_tier2` fuzz target stopped on one: the reader
//! takes the package (the central header wins), the unedited save says `edited: false`, and the bytes it writes are not the
//! file's, at that one local header. That is the spec and not a fault: an entry whose local header differs from its row in name is copied under headers made
//! anew from the row, the copy's account says so (`headers_rewritten`, the last paragraph: a copy that is not the file byte for byte always has a
//! stated cause), and the file is the copy's bytes exactly when the account is exact. What was wrong was the fuzz target's claim that an unedited save is the
//! file whatever the archive is like (its twin, `tier2_fuzz.rs`, made the same claim).
//!
//! Every input is built here in code: a package put together entry by entry with a name the local header gives that the row does not, and the artifact the
//! fuzzer kept, as the 2,829 bytes it was (a package of this crate's own writer that the fuzzer cut into, not a file of any other program).

use diacraft_container::zip::{Archive, Entry};
use diacraft_odg::{Form, Limits, OriginalPackage, WriteOptions, copy_account, copy_unchanged, read_partial, write_with};

use crate::build::{DRAWING, files, page, shape, with_manifest};
use crate::zipbuild::{Item, Opts, build, build_with};

/// The thumbnail's name, and the bytes a thumbnail of the tests holds.
const THUMB: &str = "Thumbnails/thumbnail.png";
const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// The items of a drawing package: `mimetype` stored first, the parts deflated, a thumbnail stored, the manifest last. `damage` may give an entry a local
/// header name of its own.
fn items(damage: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<Item> {
    let mut entries = files(&page("p1", &(shape("a", "1cm") + &shape("b", "2cm"))));
    entries.push((THUMB.to_owned(), PNG.to_vec()));
    let entries = with_manifest(DRAWING, entries);
    let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
    for (name, data) in &entries {
        let item = if name.ends_with(".png") { Item::stored(name, data) } else { Item::deflated(name, data) };
        items.push(match damage(name) {
            Some(local) => item.with_local_name(&local),
            None => item,
        });
    }
    items
}

/// Where the name of the local header of the entry the directory calls `name` lies in `bytes`, read from the directory by hand (APPNOTE 4.3.12, 4.3.7).
pub fn local_name_range(bytes: &[u8], name: &str) -> Option<std::ops::Range<usize>> {
    let u16_at = |at: usize| -> Option<usize> { Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?) as usize) };
    let u32_at = |at: usize| -> Option<usize> { Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize) };
    let end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let (rows, mut row) = (u16_at(end + 10)?, u32_at(end + 16)?);
    for _ in 0..rows {
        let (name_len, extra_len, comment_len) = (u16_at(row + 28)?, u16_at(row + 30)?, u16_at(row + 32)?);
        let local = u32_at(row + 42)?;
        if bytes.get(row + 46..row + 46 + name_len)? == name.as_bytes() {
            let (local_len, start) = (u16_at(local + 26)?, local + 30);
            return Some(start..start + local_len);
        }
        row += 46 + name_len + extra_len + comment_len;
    }
    None
}

/// What the unedited save of `bytes` is, by the account of its copy, asserted and said: **the file byte for byte when the account
/// is exact** (`true`), and otherwise the copy of it, which is the same entries under headers and a directory the rows give, is not the file, is itself
/// exact, and holds every part the file does (`false`). The one place a harness learns what to hold an unedited save to, so that the fuzz target and the
/// plain tests of `tier2_fuzz.rs` say the same.
pub fn unedited_save_is_the_file_or_its_copy(bytes: &[u8], saved: &[u8], limits: &Limits) -> bool {
    // A flat drawing has no entries to copy: its unedited save is its bytes. A package that was read has an account (the archive reader that makes it
    // keeps every row the drawing's reader keeps).
    let account = match Form::of(bytes) {
        Form::Flat => None,
        Form::Package => Some(copy_account(bytes, limits).unwrap_or_else(|e| panic!("a package that was read has no account of its copy: {e}"))),
    };
    match account {
        Some(account) if !account.is_exact() => {
            let copy = copy_unchanged(bytes, limits).unwrap_or_else(|e| panic!("a package that was saved cannot be copied: {e} ({account:?})"));
            assert_eq!(saved, copy, "an unedited save of an archive a copy does not keep whole is its copy: {account:?}");
            assert_ne!(saved, bytes, "an account that is not exact says the copy is not the file: {account:?}");
            assert!(copy_account(saved, limits).is_ok_and(|a| a.is_exact()), "a copy of the copy is the copy");
            // Every entry is the file's, in the bytes the archive holds: nothing is inflated, since a damaged picture's bytes are copied as they are.
            let (was, now) = (Archive::read(bytes, &limits.container).unwrap(), Archive::read(saved, &limits.container).unwrap());
            for entry in was.entries() {
                let at = now.position(&entry.name).unwrap_or_else(|| panic!("the entry {} is in the copy", entry.name));
                let copied = &now.entries()[at];
                assert_eq!(
                    (entry.method, entry.crc, entry.compressed, entry.size),
                    (copied.method, copied.crc, copied.compressed, copied.size),
                    "{}",
                    entry.name
                );
                let raw =
                    |bytes: &[u8], e: &Entry| bytes.get(e.data_offset..e.data_offset + usize::try_from(e.compressed).unwrap()).map(<[u8]>::to_vec);
                assert!(raw(bytes, entry).is_some() && raw(bytes, entry) == raw(saved, copied), "{}: the bytes are the file's", entry.name);
            }
            assert_eq!(was.entries().len(), now.entries().len(), "and there are no others");
            false
        }
        _ => {
            assert_eq!(saved, bytes, "an unedited save of an archive a copy keeps whole is the file");
            true
        }
    }
}

/// The unedited save of `bytes`, which must read: not an edit, and what [`unedited_save_is_the_file_or_its_copy`] says. Gives the saved bytes.
fn unedited_save(bytes: &[u8]) -> Vec<u8> {
    let limits = Limits::default();
    let (result, report) = read_partial(bytes, &limits);
    let doc = result.unwrap_or_else(|e| panic!("the package did not read: {} ({e})", e.class()));
    let saved = write_with(&doc, Some(&OriginalPackage { bytes, index: &report.index }), &WriteOptions::default()).unwrap();
    assert!(!saved.edited, "a model that was read and not touched is no edit");
    saved.bytes
}

/// A local name that is not the row's is written under the row's, by an unedited save that says it edited nothing, and the copy's account says
/// so (one entry rewritten, nothing dropped or reordered). Names of the length of the row's, shorter, longer, empty, another entry's and bytes no text is.
#[test]
fn a_local_name_other_than_the_rows_is_mended_by_an_unedited_save_and_the_account_says_so() {
    let limits = Limits::default();
    let long = vec![b'x'; 300];
    let local_names: [(&str, &[u8]); 6] = [
        ("cut to four bytes and noise", b"Thum\x00\x00\x00\x02i\x01<\x01\x01\n<S\nhRow N="),
        ("shorter", b"Thumbnails/t"),
        ("longer", b"Thumbnails/thumbnail.png.bak"),
        ("empty", b""),
        ("another entry's", b"content.xml"),
        ("bytes that are no text", &[0xFF, 0xFE, 0x80, 0x00, 0x41]),
    ];
    for (what, local) in local_names.iter().copied().chain([("three hundred bytes", long.as_slice())]) {
        for target in [THUMB, "content.xml", "styles.xml", "META-INF/manifest.xml"] {
            if local == target.as_bytes() {
                continue; // The row's own name is no disagreement.
            }
            let bytes = build(&items(&|name| (name == target).then(|| local.to_vec())));
            let account = copy_account(&bytes, &limits).unwrap_or_else(|e| panic!("{what} on {target}: no account: {e}"));
            assert_eq!(
                (account.rewritten, account.exact, account.rows_reordered, account.dropped.total()),
                (1, 6, false, 0),
                "{what} on {target}: {account:?}"
            );
            assert!(!account.is_exact(), "{what} on {target}");
            let saved = unedited_save(&bytes);
            assert_ne!(saved, bytes, "{what} on {target}: the save is the mended copy");
            assert!(!unedited_save_is_the_file_or_its_copy(&bytes, &saved, &limits));
            // The save has the row's name in the local header, where the file had the other, and the same entry's data.
            let at = local_name_range(&saved, target).unwrap_or_else(|| panic!("{what} on {target}: no row"));
            assert_eq!(saved.get(at), Some(target.as_bytes()), "{what} on {target}: the local header names the entry as its row does");
        }
    }
}

/// The same name length as the row's: the save differs from the file at that one local name, and nowhere else.
#[test]
fn a_local_name_of_the_rows_length_changes_the_save_at_that_name_alone() {
    let local = vec![b'~'; THUMB.len()];
    let bytes = build(&items(&|name| (name == THUMB).then(|| local.clone())));
    let saved = unedited_save(&bytes);
    assert_eq!(saved.len(), bytes.len());
    let at = local_name_range(&bytes, THUMB).unwrap();
    assert_eq!(bytes.get(at.clone()), Some(local.as_slice()));
    let differing: Vec<usize> = (0..bytes.len()).filter(|&i| bytes.get(i) != saved.get(i)).collect();
    assert_eq!(differing, at.collect::<Vec<_>>(), "only the bytes of the damaged name differ");
}

/// The account counts each entry apart: two damaged names are two rewritten, and a drawing that has bytes a copy drops besides says both.
#[test]
fn each_damaged_local_name_is_one_rewritten_entry_and_what_a_copy_drops_is_counted_apart() {
    let limits = Limits::default();
    let damage = |name: &str| (name == THUMB || name == "styles.xml").then(|| b"?".to_vec());
    let bytes = build(&items(&damage));
    let account = copy_account(&bytes, &limits).unwrap();
    assert_eq!((account.rewritten, account.exact, account.dropped.total()), (2, 5, 0), "{account:?}");
    let saved = unedited_save(&bytes);
    assert!(!unedited_save_is_the_file_or_its_copy(&bytes, &saved, &limits));
    // And with 16 bytes after the data of `content.xml` that no header announces, and an archive comment of 9.
    let mut items = items(&damage);
    for item in &mut items {
        if item.name == b"content.xml" {
            item.after_data = vec![0xAB; 16];
        }
    }
    let bytes = build_with(&items, false, &Opts { comment: b"a comment".to_vec(), ..Opts::default() });
    let account = copy_account(&bytes, &limits).unwrap();
    assert_eq!(
        (account.rewritten, account.dropped.unannounced, account.dropped.archive_comment, account.dropped.total()),
        (2, 16, 9, 25),
        "{account:?}"
    );
    let saved = unedited_save(&bytes);
    assert!(!unedited_save_is_the_file_or_its_copy(&bytes, &saved, &limits));
}

/// The control: an archive whose local names are the rows' is saved as the file, byte for byte, by the same check, so that the check above is not one
/// that any save passes.
#[test]
fn an_archive_whose_headers_agree_is_saved_as_the_file() {
    let limits = Limits::default();
    let bytes = build(&items(&|_| None));
    assert!(copy_account(&bytes, &limits).unwrap().is_exact());
    let saved = unedited_save(&bytes);
    assert_eq!(saved, bytes);
    assert!(unedited_save_is_the_file_or_its_copy(&bytes, &saved, &limits));
    // The mend of a damaged local name is the honest archive it was made from; and the check refuses the file offered where its copy is due (an echo), and a
    // save that is not the file where the file is due.
    let damaged = build(&items(&|name| (name == THUMB).then(|| b"x".to_vec())));
    assert_eq!(unedited_save(&damaged), bytes, "the copy writes the name the row has");
    assert!(std::panic::catch_unwind(|| unedited_save_is_the_file_or_its_copy(&damaged, &damaged, &Limits::default())).is_err());
    let cut = bytes.get(..bytes.len() - 1).unwrap().to_vec();
    assert!(std::panic::catch_unwind(|| unedited_save_is_the_file_or_its_copy(&bytes, &cut, &Limits::default())).is_err());
}

/// The artifact `odg_tier2-crash-c3893a76f7f31df07c8eb549c39721937282f2fb`, as the fuzzer kept it: seven entries, the local header of
/// `Thumbnails/thumbnail.png` named `Thum` and 20 bytes of noise where its row has the 24 bytes of the name.
const ARTIFACT: &[&str] = &[
    "504b03040a0000000000000021009f032ec42b0000002b000000080000006d696d65747970656170706c69636174696f",
    "6e2f766e642e6f617369732e6f70656e646f63756d656e742e6772617068696373504b03041400000008000000210066",
    "f1c0db6f030000781100000b000000636f6e74656e742e786d6ce5586daf9b3614fede5fc13ca9df8000c96dc342aa75",
    "53a549dbd5d5b66ed2be4c8e3160d5d8cc3679f9f7b3b1c9755ad1d0dbaaba6d1389d8e7cde77938c7b6b279716c69b0",
    "c74212ce0a90440b1060867849585d80d77fbe0a9f8317db271b5e5504e1bce4a86f315321e24ce9df407b33995b6d01",
    "7ac1720e259139832d96b94239ef301bbd72df3a1fd6b212a94e74b6fb60ec7b2b7c54739d8dadef5b0a7898eb6b6c35",
    "2dbe7bc5e73a1f250d2bae796b3ba8c8ee1281dcd7b3d1efeb8920474ad89b02344a75791c1f0e87e890455cd471b25e",
    "afe3417b468dce765d2fe86055a218536c5692711225f168db6205e726676cfd947495546436346b7df166e16e7e5d0c",
    "c697ef362be7bfdbacf47d3b81a5d66a964d5bcc8be1fb5cbc5d244837bb42adf5050e029180958b40785413d5f4bb88",
    "33fcef8ea868d4e7bab631338d0c02d7635e6367603b7671a59b37ac20c261891195db4d3ca57072d82bde6a5c281c9a",
    "4f2b6c130ecfc08e0da602d422014e50c196d09311c1ae21088c4e6e1e7642a3178a601998ceca2b42a92e6dca4501be",
    "cfb29b9bf51adc6b0a203925a5934825f81b3cca628dc04b683abbbb7792eba080434226c834a8f431834a1f082a7bcc",
    "a0b207825a3e6650cb07825a7d7e50c17957b9df433c21d13b6b927a0284299505784ad50f7f3418abad19fda485c16d",
    "f1f4bf9eeb0997ca8e82bf9ce8b99bbf76f35f6e9de095130ca1a2f5777f93523556140f9163bbc80770bf7a2ff7f1e4",
    "4ee7143b5e9ece137713d86e06e23a58634ba15dcacc13476a0ba5c22234a2d06a7fc615eca902db2781fed808a8977a",
    "d95036b0bb8824cd7eb5aff3a3261bb576ac33cfc6f1c1d05280e5386f30a91b7d4ea4d14a8bec0ac166b8f374f1381d",
    "e263d640867019d698eb535b9c06ff3dc187975cafb60816419adc2cdcd38151a74e2725305290d51403177213bf03c2",
    "3153fb60ee2011603b0578a03bf40e11bba24e3df45577a386c213d645ad7f78af80bfce8f57b8190586d485c76ab28a",
    "9e19d6ce7c7d34538c33dd91a4258aecb1d39cc3e91b9c4eefb7b3fbaf7e18f71cbf76f64f703b94ea14dbd7484d2749",
    "4daf92faf282d4ec6d52339fd4954f6af2e5926a656397d73359ce2659ce3e65e9a6df52e52e27395d7ecaca7df65516",
    "ee35725793e4aeae92fb3b862516f319f6ab36fdb2e93547fafdbde17c23882fee0bf1c4ff26dbff01504b0304140000",
    "00080000002100e980a02115020000d00600000a0000007374796c65732e786d6c8d55cb929b3010bce72b5cba03f1fa",
    "9058b5782fa9dc529543724e0931606541434983f1fe7d241eb6f023a51beae9d6685a33e2f5eddc369b1318ab50e76c",
    "9b7e661bd0124ba5eb9cfdfef53df9cade0e9f5eb1aa94045ea2ec5bd09458fa68c06e9c585b3e0573d61bcd515865b9",
    "162d584e9263077a11f190cdc75413326e162b1fc9a19ae04cb162cf0db5a51143acd6739d2ba1bcc258f1d936498589",
    "c4b613a48a7505f65447577faa9f6c726e947ecfd991a8e359360c433aec523475b6ddeff7d918bd542d2fbcae37cdc8",
    "2a65060df84c36dba6db6ce1b64022f6709e1b1e49a2ae547469137b75b3a288ef8b91bcbedb5d197fb7bb32d47606ac",
    "8b3a97fd54c4ed116a56b72b8deaa23b7462afea50421a51cd3b284c6b45c7be4851c39f4251bac4b9eb6dd07e8ed966",
    "9eb160ae77ecb00c71856e802b2121294136f6f09a3d0bccf834ec57deb29e97a2276c5dd9325902d38c76a286a4111f",
    "d8d366427ce139fbf963cb1e7092ce383f0c29f7ae54c85b616aa513c2ce1d5fb62cc00a2497f10e6ec09b74031a551f",
    "0374cc37a8928e397b59814798982ffbf48bc7e7f319e53c41a3aeddd0a1212314b1cc397257c5d5a57b5be6402b2c81",
    "b9a0fe5de14eec21a070bd193f27d3a6dd7dcec7f142c8f7da60afcb180e167f41927d4e75c348069bff305a10b637e0",
    "de151859d96d1d933573b1dea155137c834af40d5d7c0efae0da25d9d5cd1bd3b2c73fa4c33f504b0304140000000800",
    "0000210097282ffdbf000000540100001500000044696143726166742f646f63756d656e742e786d6c8d90b1cac24010",
    "84fb1ffe7758a637516c34e46221083662a1b672e6d67890ecc9dd45f4ed8d851aad6c1666d9996f997c766d6abab00f",
    "d689c2281982584a67ac540adbcd62300185a8c5e8da092bdc386056fcffe5c6ead2eb63cc8c2bdb8625529724217bee",
    "155a2f99754965e3a93d249d7b7fb03179f9f81a591e58f4f828dec19577ed99cebaeab08f3902896e3ab1d6d6f70fc3",
    "8939f6f49ceb9a560a73172268a730066d15962ba4459e7edbd24fe02f0f80ac51987ec63d7b28ee504b030414000000",
    "0800000021009ec0630a9e00000030010000080000006d6574612e786d6c8d8f3d0ec2300c85f79ea2cadea688055969",
    "3a8038011c204add2a12b15192228e4f7fa8541003abfd7dcfcfaa79fa5bfec0101d532d7665257224cbada3be16d7cb",
    "b9388846678abbce598496ede09152e131997c5429c2b2aac51008d84417818cc708c902df915605b634cc8796c914f5",
    "af3db18bfbced934df0bbdd69c30ad66b847c26012077d72e6184c9764558e094a7ead95fc90e5af8f75f602504b0304",
    "14000000080000002100f54075840e0000000c000000180000005468756d0000000269013c01010a3c530a68526f7720",
    "4e3d22616c3c2f5346696c6c6565743e504b030414000000080000002100be9375fdfb0000001c030000150000004d45",
    "54412d494e462f6d616e69666573742e786d6cad92416e84300c45f79c02655b415a755345c02c5af504d303b8c140a4",
    "c444c48c86db378c044c5bb1409a9d1dfbfb7d472e4e5767d30b0ec1f4548a97fc59a448baaf0db5a5f83a7f666fe254",
    "258503320d06564b90461d85352dc53890ea2198a0081c06c55af51ea9eef5e89058fdee5737d29add19781555926ebc",
    "c658cca27e98b6ee66b436f3c05d29e4de90edd9616d20e3c96329c07b6b34706c9317aaf39be1fcde67de0ee03ba383",
    "90477ce89e7856c72d77d08c579673f9d0dcc093c5f0f0b11f06de076858ae6bef13fefe59ace8599b2fdaa7c378870c",
    "0fdfe9dc8dee9bc0d8207909734fed0ec4386851cef54829e4bf03af921f504b01020a000a0000000000000021009f03",
    "2ec42b0000002b0000000800000000000000000000000000000000006d696d6574797065504b01021400140000000800",
    "0000210066f1c0db6f030000781100000b0000000000000000000000000051000000636f6e74656e742e786d6c504b01",
    "02140014000000080000002100e980a02115020000d00600000a00000000000000000000000000e90300007374796c65",
    "732e786d6c504b010214001400000008000000210097282ffdbf00000054010000150000000000000000000000000026",
    "06000044696143726166742f646f63756d656e742e786d6c504b01021400140000000800000021009ec0630a9e000000",
    "300100000800000000000000000000000000180700006d6574612e786d6c504b0102140014000000080000002100f540",
    "75840e0000000c0000001800000000000000000000000000dc0700005468756d626e61696c732f7468756d626e61696c",
    "2e706e67504b0102140014000000080000002100be9375fdfb0000001c03000015000000000000000000000000002008",
    "00004d4554412d494e462f6d616e69666573742e786d6c504b05060000000007000700a90100004e0900000000",
];

#[test]
fn the_artifact_the_fuzz_target_stopped_on_is_a_package_that_is_mended_and_not_the_file() {
    let bytes: Vec<u8> =
        ARTIFACT.concat().as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect();
    assert_eq!(bytes.len(), 2829);
    let limits = Limits::default();
    let at = local_name_range(&bytes, THUMB).unwrap();
    assert_eq!(at.len(), THUMB.len());
    assert_eq!(bytes.get(at.clone()), Some(&b"Thum\x00\x00\x00\x02i\x01<\x01\x01\n<S\nhRow N="[..]), "the name the fuzzer cut");
    let account = copy_account(&bytes, &limits).unwrap();
    assert_eq!((account.rewritten, account.exact, account.rows_reordered, account.dropped.total()), (1, 6, false, 0), "{account:?}");
    let saved = unedited_save(&bytes);
    assert!(!unedited_save_is_the_file_or_its_copy(&bytes, &saved, &limits));
    assert_eq!(saved.len(), bytes.len());
    let differing: Vec<usize> = (0..bytes.len()).filter(|&i| bytes.get(i) != saved.get(i)).collect();
    assert!(!differing.is_empty() && differing.iter().all(|i| at.contains(i)), "only the damaged name differs: {differing:?}");
    assert_eq!(saved.get(at), Some(THUMB.as_bytes()));
}
