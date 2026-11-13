//! Small helpers for uploading a local file: content sniffing, image
//! dimensions, a SHA-256 for the preview, and the base64 the REST API expects
//! for `bytes` fields.
//!
//! The preview shows a file's size and SHA-256 instead of its bytes: the hash
//! identifies the exact file that will be uploaded (`shasum -a 256 <file>`
//! prints the same value) without putting hundreds of kilobytes of base64 into
//! the conversation.

/// What a file's leading bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileFormat {
    Png,
    Jpeg,
    Gif,
    Zip,
    Unknown,
}

impl FileFormat {
    /// Human-readable name for previews and errors.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            FileFormat::Png => "PNG",
            FileFormat::Jpeg => "JPEG",
            FileFormat::Gif => "GIF",
            FileFormat::Zip => "ZIP",
            FileFormat::Unknown => "unknown",
        }
    }
}

/// Identify a file by its magic bytes.
pub(crate) fn sniff_format(data: &[u8]) -> FileFormat {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        FileFormat::Png
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        FileFormat::Jpeg
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        FileFormat::Gif
    } else if data.starts_with(b"PK\x03\x04") {
        FileFormat::Zip
    } else {
        FileFormat::Unknown
    }
}

/// Pixel `(width, height)` of a PNG, GIF or JPEG, read from its header.
pub(crate) fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    match sniff_format(data) {
        // IHDR is the first chunk: width and height are big-endian u32s at 16.
        FileFormat::Png if data.len() >= 24 => Some((
            u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            u32::from_be_bytes([data[20], data[21], data[22], data[23]]),
        )),
        // Logical screen size: little-endian u16s right after the signature.
        FileFormat::Gif if data.len() >= 10 => Some((
            u32::from(u16::from_le_bytes([data[6], data[7]])),
            u32::from(u16::from_le_bytes([data[8], data[9]])),
        )),
        FileFormat::Jpeg => jpeg_dimensions(data),
        _ => None,
    }
}

/// Walk JPEG markers to the first start-of-frame and read its size.
fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut pos = 2;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            return None;
        }
        let marker = data[pos + 1];
        // Fill bytes and standalone markers carry no length.
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        // SOF0..SOF15, except DHT (C4), JPG (C8) and DAC (CC).
        let is_sof = (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker);
        if is_sof {
            if pos + 9 > data.len() {
                return None;
            }
            let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]);
            let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        if length < 2 {
            return None;
        }
        pos += 2 + length;
    }
    None
}

/// Standard base64 (RFC 4648, with padding) — the encoding proto3 JSON uses
/// for `bytes` fields.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

/// SHA-256 round constants (FIPS 180-4, section 4.2.2).
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Lowercase hex SHA-256 of `data` (FIPS 180-4).
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    // The padding above makes the length a multiple of 64, so no remainder.
    let (blocks, _) = message.as_chunks::<64>();
    for block in blocks {
        let mut w = [0u32; 64];
        let (words, _) = block.as_chunks::<4>();
        for (word, bytes) in w.iter_mut().zip(words) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for (k, word) in SHA256_K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(*word);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    state.iter().map(|word| format!("{:08x}", word)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_fips_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A million 'a's crosses many blocks.
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(input.as_bytes()), expected);
        }
        assert_eq!(base64_encode(&[0xFF, 0xFE, 0xFD]), "//79");
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0]);
        data
    }

    #[test]
    fn reads_png_gif_and_jpeg_dimensions() {
        assert_eq!(image_dimensions(&png(300, 250)), Some((300, 250)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&728u16.to_le_bytes());
        gif.extend_from_slice(&90u16.to_le_bytes());
        assert_eq!(image_dimensions(&gif), Some((728, 90)));

        // SOI, an APP0 segment, then SOF0 with height 600 and width 160.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&600u16.to_be_bytes());
        jpeg.extend_from_slice(&160u16.to_be_bytes());
        jpeg.extend_from_slice(&[0x03]);
        assert_eq!(image_dimensions(&jpeg), Some((160, 600)));

        assert_eq!(image_dimensions(b"PK\x03\x04rest"), None);
        assert_eq!(image_dimensions(b"\x89PNG"), None);
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff_format(&png(1, 1)), FileFormat::Png);
        assert_eq!(sniff_format(b"GIF87a...."), FileFormat::Gif);
        assert_eq!(sniff_format(&[0xFF, 0xD8, 0xFF, 0xDB]), FileFormat::Jpeg);
        assert_eq!(sniff_format(b"PK\x03\x04"), FileFormat::Zip);
        assert_eq!(sniff_format(b"<html>"), FileFormat::Unknown);
        assert_eq!(FileFormat::Zip.name(), "ZIP");
    }
}
