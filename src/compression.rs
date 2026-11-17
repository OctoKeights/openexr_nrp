//! zlib streams as OpenEXRCore makes and reads them (`compression.c` of OpenEXR v3.5.4), with
//! the libdeflate it comes with ([`libdeflate_nrp`]).

use crate::Error;
use libdeflate_nrp::Level;

/// `exr_compress_max_buffer_size`: the room `exr_compress_buffer` is given, which is also
/// enough for OpenEXRCore's Huffman coder's spare memory.
pub(crate) fn compress_max_buffer_size(in_bytes: usize) -> usize {
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
/// stream at `level`.
pub(crate) fn compress_buffer(level: Level, input: &[u8]) -> Vec<u8> {
    libdeflate_nrp::zlib_compress(input, level)
}

/// The level OpenEXRCore compresses at unless a file says otherwise: libdeflate's 4.
pub(crate) const DEFAULT_LEVEL: Level = match Level::new(4) {
    Some(level) => level,
    None => unreachable!(),
};

/// `exr_uncompress_buffer`: the zlib stream `input` into the start of `out`, the bytes made,
/// or why not: [`Error::CorruptChunk`] for a stream that is not one or does not end where
/// `input` does, [`Error::OutOfMemory`] for one that makes more than `out` takes.
pub(crate) fn uncompress_buffer(input: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    match libdeflate_nrp::zlib_decompress(input, out) {
        Ok(result) if result.bytes_read == input.len() => Ok(result.bytes_written),
        Ok(_) | Err(libdeflate_nrp::Error::BadData) => Err(Error::CorruptChunk),
        Err(libdeflate_nrp::Error::InsufficientSpace) => Err(Error::OutOfMemory),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_refusals() {
        let data = b"OpenEXR, OpenEXR, OpenEXR, and again OpenEXR".repeat(50);
        let stream = compress_buffer(Level::BEST, &data);
        assert!(stream.len() <= compress_max_buffer_size(data.len()));
        assert_ne!(compress_buffer(DEFAULT_LEVEL, &data), stream);
        let mut out = vec![0u8; data.len()];
        assert_eq!(uncompress_buffer(&stream, &mut out), Ok(data.len()));
        assert_eq!(out, data);
        assert_eq!(
            uncompress_buffer(&stream, &mut out[..10]),
            Err(Error::OutOfMemory)
        );
        let mut longer = stream.clone();
        longer.push(0);
        assert_eq!(
            uncompress_buffer(&longer, &mut out),
            Err(Error::CorruptChunk)
        );
        assert_eq!(compress_max_buffer_size(0), 1_835_036);
        assert_eq!(compress_max_buffer_size(usize::MAX), usize::MAX);
    }
}
