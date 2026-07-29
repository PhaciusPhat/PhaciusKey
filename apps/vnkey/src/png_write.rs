//! Minimal, dependency-free PNG encoder — just enough to write an 8-bit RGBA
//! image. Used only to build the app-icon iconset at package time (see
//! `tray::export_iconset`), so we skip an `image`/`png` crate dependency in
//! favor of the simplest valid encoding: "stored" (uncompressed) DEFLATE
//! blocks. Every PNG decoder must support them per RFC 1951, so this is
//! portable despite being hand-rolled.

use std::io;
use std::path::Path;

/// Write `rgba` (straight-alpha, `size`×`size`×4 bytes) as an 8-bit RGBA PNG.
pub(crate) fn write_png(path: &Path, size: u32, rgba: &[u8]) -> io::Result<()> {
    assert_eq!(rgba.len(), size as usize * size as usize * 4);

    let stride = size as usize * 4;
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in 0..size as usize {
        raw.push(0); // filter type: None
        raw.extend_from_slice(&rgba[row * stride..row * stride + stride]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // depth 8, color type 6 (RGBA), rest default
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    write_chunk(&mut out, b"IEND", &[]);

    std::fs::write(path, out)
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&out[start..]).to_be_bytes());
}

/// zlib-wrap `data` using only uncompressed ("stored") DEFLATE blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // CMF/FLG for a valid, dictionary-less zlib stream
    let mut i = 0;
    loop {
        let chunk_len = (data.len() - i).min(0xFFFF);
        let is_final = i + chunk_len == data.len();
        out.push(is_final as u8); // BFINAL in bit 0, BTYPE=00 (stored) in bits 1-2
        out.extend_from_slice(&(chunk_len as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk_len as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + chunk_len]);
        i += chunk_len;
        if is_final {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}
