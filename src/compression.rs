//! zlib streams as OpenEXRCore makes and reads them (`compression.c` of OpenEXR v3.5.4), with
//! the libdeflate it comes with ([`libdeflate_nrp`]).

use crate::{CORRUPT_CHUNK, Code, OUT_OF_MEMORY};

/// `exr_compress_max_buffer_size`: the room `exr_compress_buffer` is given, which is also
/// enough for OpenEXRCore's Huffman coder's spare memory.
pub fn compress_max_buffer_size(in_bytes: usize) -> usize {
    let Some(mut r) = libdeflate_nrp::zlib_compress_bound(in_bytes).checked_add(9) else {
        return usize::MAX;
    };
    let Some(extra) = r.checked_mul(130) else {
        return usize::MAX;
    };
    let extra = extra / 128;
    if extra > usize::MAX - 100 {
        return usize::MAX;
    }
    if extra > r {
        r = extra;
    }
    // in case huf is larger than zlib (`internal_exr_huf_compress_spare_bytes` and
    // `internal_exr_huf_decompress_spare_bytes`)
    r = r
        .max(in_bytes.saturating_add(1_835_036))
        .max(in_bytes.saturating_add(786_440));
    r.max(8192)
}

/// `exr_compress_buffer` with libdeflate's `libdeflate_zlib_compress`: `input` as a zlib
/// stream at `level` (a negative level is OpenEXR's default, 4; libdeflate's levels 0 and 2
/// to 9).
///
/// # Panics
///
/// For level 1 or a level above 9, which [`libdeflate_nrp`] does not port.
pub fn compress_buffer(level: i32, input: &[u8]) -> Vec<u8> {
    let level = if level < 0 { 4 } else { level };
    let level = u8::try_from(level)
        .ok()
        .and_then(libdeflate_nrp::Level::new)
        .unwrap_or_else(|| panic!("libdeflate level {level} is not ported"));
    libdeflate_nrp::zlib_compress(input, level)
}

/// `exr_uncompress_buffer`: the zlib stream `input` into the start of `out`, the bytes made,
/// or OpenEXRCore's code for why not: [`CORRUPT_CHUNK`] for a stream that is not one or does
/// not end where `input` does, [`OUT_OF_MEMORY`] for one that makes more than `out` takes.
pub fn uncompress_buffer(input: &[u8], out: &mut [u8]) -> Result<usize, Code> {
    match libdeflate_nrp::zlib_decompress(input, out) {
        Ok(result) if result.bytes_read == input.len() => Ok(result.bytes_written),
        Ok(_) | Err(libdeflate_nrp::Error::BadData) => Err(CORRUPT_CHUNK),
        Err(libdeflate_nrp::Error::InsufficientSpace) => Err(OUT_OF_MEMORY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_refusals() {
        let data = b"OpenEXR, OpenEXR, OpenEXR, and again OpenEXR".repeat(50);
        // (OpenEXR's default is 4.)
        assert_eq!(compress_buffer(-1, &data), compress_buffer(4, &data));
        let stream = compress_buffer(9, &data);
        assert!(stream.len() <= compress_max_buffer_size(data.len()));
        let mut out = vec![0u8; data.len()];
        assert_eq!(uncompress_buffer(&stream, &mut out), Ok(data.len()));
        assert_eq!(out, data);
        assert_eq!(
            uncompress_buffer(&stream, &mut out[..10]),
            Err(OUT_OF_MEMORY)
        );
        let mut longer = stream.clone();
        longer.push(0);
        assert_eq!(uncompress_buffer(&longer, &mut out), Err(CORRUPT_CHUNK));
        assert_eq!(compress_max_buffer_size(0), 1_835_036);
        assert_eq!(compress_max_buffer_size(usize::MAX), usize::MAX);
    }
}
