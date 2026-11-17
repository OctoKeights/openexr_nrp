//! Compressions of OpenEXR's `.exr` files: a native Rust port of parts of OpenEXRCore, the C
//! core of [OpenEXR](https://github.com/AcademySoftwareFoundation/openexr) v3.5.4.
//!
//! - [`dwa`]: DWAA and DWAB, DreamWorks Animation's lossy compressions, a block at a time
//!   (`internal_dwa*.{c,h}`, with the Huffman coder of `internal_huf.c` they use).
//! - [`compression`]: zlib streams as OpenEXRCore makes and reads them for those
//!   (`compression.c`), with the libdeflate it comes with ([`libdeflate_nrp`]).
//!
//! The blocks are OpenEXRCore's, byte for byte, as it makes them on an x86-64 machine with
//! AVX and F16C (any from about 2011 on), whatever machine this runs on; and every block,
//! whole or damaged, decompresses to what OpenEXRCore makes of it, or is refused with its
//! code. The rest of OpenEXR (reading and writing files, and its other compressions) is not
//! ported: [tinyexr_nrp](https://github.com/OctoKeights/tinyexr_nrp) reads and writes files
//! with these.
#![forbid(unsafe_code)]
// The code follows the original's line for line, and these are its shapes.
#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_range_contains
)]

pub mod compression;
pub mod dwa;

/// An `exr_result_t` of OpenEXRCore: why something was not done.
pub type Code = i32;
/// `EXR_ERR_OUT_OF_MEMORY`.
pub const OUT_OF_MEMORY: Code = 1;
/// `EXR_ERR_INVALID_ARGUMENT`.
pub const INVALID_ARGUMENT: Code = 3;
/// `EXR_ERR_ARGUMENT_OUT_OF_RANGE`.
pub const ARGUMENT_OUT_OF_RANGE: Code = 4;
/// `EXR_ERR_BAD_CHUNK_LEADER`.
pub const BAD_CHUNK_LEADER: Code = 22;
/// `EXR_ERR_CORRUPT_CHUNK`.
pub const CORRUPT_CHUNK: Code = 23;

/// (The README's examples, compiled and run with the tests.)
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
