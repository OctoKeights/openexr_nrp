# Verification

[Overview](../README.md) · [Guide](guide.md) · **Verification** · [Performance](performance.md)

---

openexr_nrp is checked against OpenEXRCore itself: both are run over the same synthetic
blocks and their output compared byte for byte.

## The comparison

`compare/compare.sh` builds `compare/dump_dwa.c` against the OpenEXRCore of a CMake build of
OpenEXR v3.5.4, and `examples/dump.rs` against openexr_nrp; it runs both and compares what
they print:

```sh
OPENEXR_BUILD=<its CMake build folder> compare/compare.sh <checkout of OpenEXR> [count] [seeds] [dumps]
```

Both programs generate the same blocks from each seed and print a running hash of
everything they produce:

- **dwa_write**: synthetic blocks of one to many channels, of halves, floats and unsigned
  integers, with names that exercise every classification rule, of lines (DWAA's 32 and
  DWAB's 256) and of tiles, at random levels. Each is written by OpenEXRCore as the one block
  of a file, compressed with DWAA or DWAB, and decompressed again; the hash covers the
  compressed block and what it decompresses to.

- **dwa_read**: the same blocks, each damaged in one of eleven ways (or left intact) and
  decompressed; the hash covers what each decompresses to, or the error OpenEXRCore returns.

OpenEXRCore picks its code by what the machine has; the comparison is run on an x86-64
machine with AVX and F16C, whose results openexr_nrp reproduces on every platform.

## Results

| Dump | Seeds | Blocks | Input | Result |
|---|---|---|---|---|
| dwa_write | 2 | 400,000 | 7.8 GB | Every block identical, compressed and decompressed |
| dwa_read | 2 | 400,000 | 7.7 GB | Every damaged block decompresses identically or fails with the same error |

## Tests

`cargo test` needs no C compiler. `tests/golden.rs` checks openexr_nrp against hashes
recorded from OpenEXRCore (`tests/data/`, 800 blocks written and 1,200 read), unit tests
check the half conversions, the compression levels, round trips and the refusal of damaged
blocks, and the examples in the README and
the Guide are run as tests. `tests/data/README.md` explains how to record the hashes again.
