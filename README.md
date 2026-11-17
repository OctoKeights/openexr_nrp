# openexr_nrp

**OpenEXR's DWAA and DWAB compressions in pure, safe Rust, bit-identical to OpenEXRCore.**

**Overview** · [Guide](docs/guide.md) · [Verification](docs/verification.md) · [Performance](docs/performance.md)

---

openexr_nrp is a native Rust port of DWAA and DWAB, DreamWorks Animation's lossy
compressions for `.exr` files, as OpenEXRCore, the C core of
[OpenEXR](https://github.com/AcademySoftwareFoundation/openexr) v3.5.4, implements them. It
compresses and decompresses a block of an image's lines at a time, producing OpenEXRCore's
blocks byte for byte as OpenEXRCore makes them on x86-64, whatever machine it runs on.

## Highlights

- **Bit-identical blocks.** The classification of channels, the conversion to Y'CbCr, the
  forward and inverse DCT, the quantisation, and the RLE, zlib and Huffman coding all match
  OpenEXRCore on an x86-64 machine with AVX and F16C, NaN payloads included.

- **Exact decompression.** Every block, intact or damaged, decompresses to what OpenEXRCore
  makes of it, or fails with the same error.

- **Portable.** The arithmetic OpenEXRCore does with SIMD instructions is reproduced in plain
  Rust, so the blocks are the same on every platform; OpenEXRCore's own differ on machines
  without AVX and F16C.

- **One dependency:** [libdeflate_nrp](https://github.com/OctoKeights/libdeflate_nrp), the
  port of the libdeflate OpenEXR bundles. No C compiler, no build script.

- **No `unsafe` code.** Enforced by `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
openexr_nrp = "0.2"
```

## Quick start

```rust
use openexr_nrp::PixelType;
use openexr_nrp::dwa::{AcCompression, Channel, compress, decompress};

// Three half-float channels, in the order of their names, as a file stores them.
let channels: Vec<Channel> = ["B", "G", "R"]
    .iter()
    .map(|&name| Channel { name, pixel_type: PixelType::Half, linear: false })
    .collect();

// A block of 16 lines of 16 pixels: each line holds each channel's values in turn.
let (width, lines) = (16, 16);
let mut raw = Vec::new();
for y in 0..lines {
    for c in 0..3 {
        for x in 0..width {
            let half = 0x3400u16 + (x * 32 + y * 16 + c * 64) as u16; // 0.25 to about 0.75
            raw.extend_from_slice(&half.to_le_bytes());
        }
    }
}

// OpenEXR's default level, 45.
let block = compress(&raw, &channels, width, lines, 45.0, AcCompression::Huffman).unwrap();
assert!(block.len() < raw.len());

// Back again, close to the original: DWA is lossy.
let mut back = vec![0u8; raw.len()];
decompress(&block, &channels, width, lines, &mut back).unwrap();
```

## Scope

DWAA and DWAB blocks, both ways. Reading and writing `.exr` files and OpenEXR's other
compressions are not ported; [tinyexr_nrp](https://github.com/OctoKeights/tinyexr_nrp) reads
and writes files with DWAA and DWAB through this crate.

## Licence

BSD 3-Clause, as OpenEXR is; see [`LICENSE`](LICENSE), which keeps OpenEXR's copyright
notice.

Bug reports and contributions are welcome, above all any block that openexr_nrp compresses
or decompresses differently from OpenEXRCore.
