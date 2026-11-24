//! Kani proof harnesses for the byte code of `cargo xtask site --web`: the reader of a WebAssembly number (`site::app::leb128`), the test of
//! whether a module declares a shared memory (`wasm_shared_memory`), and the sizes the report writes (`size_parts`, what `size` writes). Run with `cargo xtask kani`
//! (`docs/development.md`, Proofs). The crate is a library and a binary; the site's checks are the binary's, so this file is declared in `src/main.rs`.
//!
//! **Not proved here:** the rest of the site's checks (they read the files of a folder, parse HTML and TOML and match names: strings and the file system); the other
//! commands of the crate (they run `cargo`, `git` and other tools). Their tests in `tests/it` and `src/site/*tests.rs` hold them.

use crate::site::{leb128, size_parts, wasm_shared_memory};

/// `leb128` (an unsigned LEB128 number of a WebAssembly module, 7 bits a byte, the high bit saying another byte follows) for **every** slice of up to eleven
/// bytes and every place to start from up to one past its end: a number exactly when one of the first ten bytes from the place, all within the slice, has no high bit and the
/// bytes before it all have one, and the tenth, if it is that byte, holds no more than the top bit of a 64-bit number; then the number is the 7-bit groups, lowest first, and
/// the place is one past that byte, 1 to 10 bytes on and within the slice. It never reads past the slice nor panics.
#[kani::proof]
#[kani::unwind(13)]
fn a_webassembly_number_is_its_seven_bit_groups_read_from_one_to_ten_bytes_within_the_slice() {
    let all: [u8; 11] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 11);
    let bytes = &all[..len];
    let start: usize = kani::any();
    kani::assume(start <= 12);
    let mut at = start;
    let got = leb128(bytes, &mut at);
    // The law's own reading: the first byte from `start` with no high bit, among ten.
    let mut end = None;
    for k in 0..10 {
        match bytes.get(start + k) {
            None => break,
            Some(b) if b & 0x80 == 0 => {
                end = Some(k);
                break;
            }
            Some(_) => {}
        }
    }
    let fits = end.is_some_and(|k| k < 9 || bytes[start + 9] & 0x7f <= 1);
    assert!(got.is_some() == fits);
    if let (Some(v), Some(k)) = (got, end) {
        assert!(at == start + k + 1 && at <= len);
        let mut want: u128 = 0;
        for i in 0..=k {
            want |= u128::from(bytes[start + i] & 0x7f) << (7 * i);
        }
        assert!(u128::from(v) == want);
    }
    if got.is_none() {
        assert!(at <= start.max(len));
    }
    kani::cover!(got.is_some() & (at == start + 1), "a number of one byte");
    kani::cover!(got.is_some() & (at == start + 10), "a number of ten bytes");
    kani::cover!(got.is_none() & (len >= start + 10), "a tenth byte past the top bit, or an eleventh");
    kani::cover!(got.is_none() & (len < start + 10), "a number cut off");
}

/// `wasm_shared_memory` for a memory section of one memory with **every** flags byte and every bytes of its least and most size: shared exactly when the flags' bit 0x02 is
/// set and the limits are whole within the section (a least size of one byte, and a most size of one byte where the flags' bit 0x01 says there is one).
#[kani::proof]
#[kani::unwind(12)]
fn a_memory_section_declares_a_shared_memory_exactly_when_its_flags_say_so() {
    let (flags, least, most): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    let module: [u8; 14] = [0, b'a', b's', b'm', 1, 0, 0, 0, 5, 4, 1, flags, least, most];
    let got = wasm_shared_memory(&module);
    let whole = least < 0x80 && (flags & 0x01 == 0 || most < 0x80);
    if flags & 0x02 == 0 {
        assert!(got != Ok(true));
    } else if whole {
        assert!(got == Ok(true));
    }
    kani::cover!(got == Ok(true), "a shared memory");
    kani::cover!((got == Ok(false)) & whole, "a memory not shared");
}

/// The same for an import of one memory, with an empty module and field name: shared exactly when the flags' bit 0x02 is set, where the limits are whole.
#[kani::proof]
#[kani::unwind(12)]
fn an_imported_memory_is_shared_exactly_when_its_flags_say_so() {
    let (flags, least, most): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    let module: [u8; 17] = [0, b'a', b's', b'm', 1, 0, 0, 0, 2, 7, 1, 0, 0, 2, flags, least, most];
    let got = wasm_shared_memory(&module);
    let whole = least < 0x80 && (flags & 0x01 == 0 || most < 0x80);
    if flags & 0x02 == 0 {
        assert!(got != Ok(true));
    } else if whole {
        assert!(got == Ok(true));
    }
    kani::cover!(got == Ok(true), "a shared memory imported");
    kani::cover!((got == Ok(false)) & whole, "a memory imported not shared");
}

/// `size_parts` (the sizes of the app's files as the site's report writes them) for **every** count of bytes: the whole mebibytes, and the hundredths of one
/// past them, cut: below 100, so the two digits the report writes are all of them.
#[kani::proof]
fn a_size_is_its_whole_mebibytes_and_the_hundredths_of_one_past_them() {
    let bytes: u64 = kani::any();
    let (whole, hundredths) = size_parts(bytes);
    const MIB: u64 = 1 << 20;
    assert!(whole == bytes >> 20 && hundredths < 100);
    let rest = u128::from(bytes & (MIB - 1)) * 100;
    assert!(u128::from(hundredths) * u128::from(MIB) <= rest && rest < u128::from(hundredths + 1) * u128::from(MIB));
    kani::cover!(hundredths == 99, "the last hundredth");
    kani::cover!((whole == 25) & (hundredths == 0), "25 MiB to the byte and a little");
}
