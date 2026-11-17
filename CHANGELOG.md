# Changelog

## 0.2.0

- **Breaking: an idiomatic API.** A channel's type is a `PixelType` (`Uint`, `Half`,
  `Float`) in place of OpenEXR's numbers, and `Channel::p_linear` is `Channel::linear`.
  Errors are an `Error` enum (`OutOfMemory`, `InvalidArgument`, `ArgumentOutOfRange`,
  `BadChunkLeader`, `CorruptChunk`), whose `code()` gives OpenEXRCore's number, in place of
  integer codes. `dwa::compress` takes an `AcCompression` (`Huffman` or `Deflate`) in place
  of a `bool`, and returns a `Result`, refusing a raw block of the wrong size;
  `dwa::uncompress` is `dwa::decompress`. The `compression` module is no longer public.

- **Faster.** Compression is 15% to 17% faster and decompression 34% to 39% (see
  [Performance](docs/performance.md)). Output is unchanged, checked against OpenEXRCore on
  400,000 blocks written and 400,000 damaged blocks read.

- **Rewritten as idiomatic Rust.** The code no longer mirrors the original's C line by line.

- **Documentation.** Every public item is documented, the README is a short overview, and
  the detail is in [Guide](docs/guide.md), [Verification](docs/verification.md) and
  [Performance](docs/performance.md).

## 0.1.0

- First release: the port of OpenEXRCore's DWAA and DWAB compressions, with its zlib streams, moved out of [rustbcx](https://github.com/OctoKeights/rustbcx), where
  it was `rustbcx::dwa` and the OpenEXR side of `rustbcx::libdeflate`.
