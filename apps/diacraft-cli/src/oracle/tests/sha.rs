//! The file's identifier: SHA-256, against the published vectors and the block boundaries.

use super::super::sha256::sha256_hex;

#[test]
fn sha256_matches_the_published_vectors_and_the_block_boundaries() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    for (n, digest) in [
        (1usize, "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5"),
        (55, "e7313d333c272e639f790978283f9eb392e843d0f29b7016828bb1daa4aac70b"),
        (56, "4324d65f3c103567f5589c710bc08f8523f929a9272e3af36fc968e52abc6c27"),
        (63, "81c80242132f230c3bd41b3e63bbcff16107339549214a99614ff26664625055"),
        (64, "39e3d7b6b5d075d37d053ad89b24b41bef4f3c29760c84447cab3f3be1882241"),
        (65, "aacca6ff74fdbb296d165a45cecfa04e5127bc008770fbbdd48006f2d2fae95e"),
        (119, "9ce7368e4daf32341631b492e80359dc9f594b48453cd0dd5bf0b19279cc177e"),
        (120, "7836b787757e95e58b3ca5aec90b1b004e8deba1e50e9675af9cabf1a13a04b5"),
        (128, "d2742f1f4ac6bb7ca2b239ee18402ba8b3f9f8e652d2a72973c2b9ba11c08cf6"),
        (1000, "1e9bc38cbf860b9ec31918b065f9b52476c549a782e0e7990bed8ce3868d2371"),
    ] {
        let data: Vec<u8> = (0..n).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        assert_eq!(sha256_hex(&data), digest, "{n} bytes");
    }
    assert_eq!(sha256_hex(&vec![b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}
