# Guide

[Overview](../README.md) · **Guide** · [Verification](verification.md) · [Performance](performance.md)

---

- [Blocks](#blocks)
- [How channels are compressed](#how-channels-are-compressed)
- [Compressing](#compressing)
- [Decompressing](#decompressing)
- [Correspondence with OpenEXRCore](#correspondence-with-openexrcore)
- [Differences from OpenEXRCore](#differences-from-openexrcore)

## Blocks

A `.exr` file compressed with DWAA stores its image in blocks of 32 lines, and with DWAB in
blocks of 256; the last block of an image, or a tile, may be shorter. Both use the same
coding. A block's uncompressed form, which `compress` takes and `decompress` produces, holds
its lines one after another, and each line holds each channel's values in turn, in the
order of `channels` (a file's order: sorted by name). Values are little-endian, of each
channel's `PixelType`: 16-bit halves, 32-bit floats or 32-bit unsigned integers. Every
channel is taken to be sampled at every pixel.

## How channels are compressed

A channel's name, after its last `.`, and its type decide how it is compressed:

| Suffix | Types | Compression |
|---|---|---|
| `R`, `G` and `B` of one layer | Half or float | Converted to Y'CbCr together, then a lossy DCT |
| `R`, `G` or `B` alone, `Y`, `BY`, `RY` | Half or float | A lossy DCT |
| `A` | Any | Run-length coding, then zlib: lossless |
| Anything else | Any | zlib: lossless |

The lossy channels are converted to a perceptual scale before the DCT, unless the file marks
them as linear (`Channel::linear`). Their quantised coefficients are run-length coded; the
AC coefficients are then Huffman coded, or, as OpenEXR does for the tiles of DWAA,
compressed with zlib (`AcCompression::Deflate`).

A block records the rules it was compressed with, so a block written by another
implementation with other rules decompresses as that implementation intended. Blocks of
version 1 of the format, which record none, use OpenEXR's older, case-insensitive rules.

## Compressing

`compress` takes the level of the file's `dwaCompressionLevel` attribute, 45 by default:
higher levels quantise more coarsely, making smaller and lossier blocks. A block that does
not come out smaller is returned unchanged, as OpenEXR stores it.

```rust
use openexr_nrp::{Error, PixelType};
use openexr_nrp::dwa::{AcCompression, Channel, compress};

let channels = [Channel { name: "Y", pixel_type: PixelType::Float, linear: false }];
let raw = vec![0u8; 8 * 8 * 4];
let block = compress(&raw, &channels, 8, 8, 45.0, AcCompression::Huffman).unwrap();

// The raw block must be the size the channels, width and lines make.
assert_eq!(
    compress(&raw[1..], &channels, 8, 8, 45.0, AcCompression::Huffman),
    Err(Error::InvalidArgument)
);
```

## Decompressing

`decompress` writes a block's lines into a buffer of its uncompressed size. A block that
OpenEXRCore refuses fails with the `Error` it reports: `Error::CorruptChunk` for most damage,
`Error::BadChunkLeader` for a later version of the format, and `Error::OutOfMemory` or
`Error::ArgumentOutOfRange` where OpenEXRCore gives those. `Error::code` gives OpenEXRCore's
number for each.

## Correspondence with OpenEXRCore

| OpenEXRCore | openexr_nrp |
|---|---|
| `DwaCompressor_compress` (through `internal_exr_apply_dwaa` and `_dwab`) | `dwa::compress` |
| `DwaCompressor_uncompress` (through `internal_exr_undo_dwaa` and `_dwab`) | `dwa::decompress` |
| `exr_attr_chlist_entry_t` | `dwa::Channel` |
| `EXR_PIXEL_UINT`, `EXR_PIXEL_HALF`, `EXR_PIXEL_FLOAT` | `PixelType::Uint`, `Half`, `Float` |
| `STATIC_HUFFMAN`, `DEFLATE` | `AcCompression::Huffman`, `AcCompression::Deflate` |
| `exr_result_t` | `Error` |

## Differences from OpenEXRCore

- **A block at a time.** Blocks are given as bytes with a list of channels, rather than
  through OpenEXRCore's encode and decode pipelines and its chunk tables.

- **The same arithmetic everywhere.** OpenEXRCore chooses its code by the processor: with
  AVX and F16C it uses SIMD conversions and transforms whose rounding and NaN handling differ
  from its scalar code. This crate always reproduces the x86-64 AVX and F16C results.

- **Fixed lookup tables.** OpenEXRCore builds its tables for the perceptual scale with the C
  library's `powf` and `logf`; this crate embeds the tables glibc's functions produce
  (`src/dwa/to_linear.bin` and `src/dwa/to_nonlinear.bin`).
