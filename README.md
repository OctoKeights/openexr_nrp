# openexr_nrp

**OpenEXR's DWAA and DWAB in pure, safe Rust: a native Rust port of OpenEXRCore's.**

openexr_nrp is a line-for-line port of parts of OpenEXRCore, the C core of
[OpenEXR](https://github.com/AcademySoftwareFoundation/openexr) v3.5.4: DWAA and DWAB,
DreamWorks Animation's lossy compressions for `.exr` files, a block at a time, with the zlib
streams OpenEXRCore makes for them. Its blocks are OpenEXRCore's, byte for byte, as it makes
them on x86-64, whatever machine the port runs on.

## Highlights

- **Bit-identical blocks.** The classification of channels, the conversion to Y'CbCr, the
  forward and inverse DCT, the quantization, the RLE, ZIP and Huffman coding, and the
  storing of a block that does not come out smaller, as OpenEXRCore has them on an x86-64
  machine with AVX and F16C (any from about 2011 on).

- **Every block read.** A damaged block decompresses to what OpenEXRCore makes of it, or is
  refused with its code.

- **One dependency of its own:** [libdeflate_nrp](https://github.com/OctoKeights/libdeflate_nrp),
  the port of the libdeflate OpenEXR comes with. No C compiler, no build script.

- **No `unsafe` code.** Enforced by `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
openexr_nrp = "0.1"
```

## Quick start

```rust
use openexr_nrp::dwa::{Channel, compress, uncompress};

// A block of 16 lines of 16 pixels, each line its channels one after the other (in the order
// of their names, as a file has them): here B, G and R, as little-endian halves.
let channels: Vec<Channel> = ["B", "G", "R"]
    .iter()
    .map(|&name| Channel { name, pixel_type: 1, p_linear: false })
    .collect();
let (width, lines) = (16, 16);
let mut raw = Vec::new();
for y in 0..lines {
    for c in 0..3 {
        for x in 0..width {
            // Halves from 0.25 to about 0.75.
            let value = 0x3400u16 + (x * 32 + y * 16 + c * 64) as u16;
            raw.extend_from_slice(&value.to_le_bytes());
        }
    }
}

// DWAA's level 45, OpenEXR's default; its AC coefficients by Huffman coding.
let block = compress(&raw, &channels, width, lines, 45.0, false);
assert!(block.len() < raw.len());

// Back, nearly as they were (DWA is lossy).
let mut back = vec![0u8; raw.len()];
uncompress(&block, &channels, width, lines, &mut back).unwrap();
```

## What is ported

| Module | Original | What it does |
|---|---|---|
| `dwa` | `internal_dwa.c`, `internal_dwa_compressor.h`, `internal_dwa_decoder.h`, `internal_dwa_encoder.h`, `internal_dwa_channel.h`, `internal_dwa_classifier.h`, `internal_dwa_helpers.h`, `internal_dwa_simd.h`, `internal_dwa_table_init.c`, `internal_huf.c` | A block of lines to DWAA or DWAB (`compress`) and back (`uncompress`) |
| `compression` | `compression.c` | zlib streams as OpenEXRCore makes them (`compress_buffer`, `compress_max_buffer_size`) and reads them (`uncompress_buffer`) |

The lookup tables OpenEXRCore makes for the conversion of non-linear values with the C
library's `powf` and `logf` are as glibc makes them, recorded in `src/dwa/to_linear.bin` and
`src/dwa/to_nonlinear.bin`. Every channel is taken to be sampled at every pixel.

The rest of OpenEXR is not ported: reading and writing files and its other compressions.
[tinyexr_nrp](https://github.com/OctoKeights/tinyexr_nrp) reads and writes files with these.

## Differences from the original

The code is translated from C to Rust. A block is given as bytes with a list of its channels,
rather than through OpenEXRCore's encode and decode pipelines. The arithmetic OpenEXRCore does
with AVX and F16C instructions on x86-64 is done here in plain Rust, in the same order and with
the same rounding, so that the port makes those blocks on any machine; OpenEXRCore's own
blocks differ on machines without them. What they compute is kept: the same block, byte for
byte, from the same lines; the same lines from every block; and the same code for a block
OpenEXRCore refuses.

## Verification

`compare/compare.sh` builds `compare/dump_dwa.c` against the OpenEXRCore of a CMake build of
OpenEXR, and `examples/dump.rs` against the port; it runs both over the same made-up data and
compares what they print byte for byte:

```sh
OPENEXR_BUILD=<its CMake build folder> compare/compare.sh <checkout of OpenEXR> [count] [seeds] [dumps]
```

| Dump | What it covers | Last full run |
|---|---|---|
| `dwa_write` | Made-up blocks of one to six channels (halves, floats and integers; named as DWA's rules expect and otherwise; now and then linear), up to 400 pixels wide and up to 32 or 256 lines, of eight kinds of values, among them every bit pattern; each written by OpenEXRCore as the only block of a file of lines or of one tile, compressed with DWAA or DWAB at a made-up level, and decompressed | 400,000 blocks (7.7 gigabytes of lines) compressed to identical blocks (4.2 gigabytes): 262,000 by DWAA or DWAB and 138,000 stored as they are, not coming out smaller; every block decompressed to the same lines |
| `dwa_read` | The same blocks, damaged in one of eleven ways (bytes flipped, cut short, a counter or the compression of the AC coefficients changed, the rules of the channels changed or made up, an older version, bytes after the end, made-up counters and data) and decompressed | 400,000 damaged blocks (4.3 gigabytes): 69,000 decompressed to the same lines and 331,000 refused with the same code |

OpenEXRCore was built by gcc at `-O3`, as its CMake build has it. In addition, files written
with the port (through tinyexr_nrp) were read by OpenEXR, and files written by OpenEXR by the
port, to the same values.

`cargo test` needs no C compiler: the tests check the port against hashes recorded from
OpenEXRCore (`tests/data`).

## Licence

As OpenEXR is: BSD 3-Clause ([LICENSE](LICENSE), which keeps OpenEXR's copyright notice).
