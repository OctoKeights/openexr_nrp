//! OpenEXR's DWAA and DWAB compressions in pure, safe Rust: a native port of OpenEXRCore's,
//! the C core of [OpenEXR](https://github.com/AcademySoftwareFoundation/openexr) v3.5.4.
//!
//! [`dwa::compress`] compresses a block of an image's lines as a `.exr` file stores it, and
//! [`dwa::decompress`] decompresses one. The blocks are OpenEXRCore's, byte for byte, as it
//! makes them on an x86-64 machine with AVX and F16C (any from about 2011 on), whatever
//! machine this runs on; and every block, intact or damaged, decompresses to what OpenEXRCore
//! makes of it, or fails with the [`Error`] OpenEXRCore reports.
//!
//! The rest of OpenEXR, reading and writing files and its other compressions, is not ported:
//! [tinyexr_nrp](https://github.com/OctoKeights/tinyexr_nrp) reads and writes files with
//! these.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compression;
pub mod dwa;

use core::fmt;

/// The type of a channel's values, as a `.exr` file records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelType {
    /// 32-bit unsigned integers (`EXR_PIXEL_UINT`).
    Uint = 0,
    /// 16-bit floats, halves (`EXR_PIXEL_HALF`).
    Half = 1,
    /// 32-bit floats (`EXR_PIXEL_FLOAT`).
    Float = 2,
}

impl PixelType {
    /// The pixel type a file records as `code`: 0, 1 or 2.
    pub const fn from_code(code: i32) -> Option<PixelType> {
        match code {
            0 => Some(PixelType::Uint),
            1 => Some(PixelType::Half),
            2 => Some(PixelType::Float),
            _ => None,
        }
    }

    /// The number a file records for the pixel type.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The bytes of one value.
    pub const fn bytes(self) -> usize {
        match self {
            PixelType::Half => 2,
            PixelType::Uint | PixelType::Float => 4,
        }
    }
}

/// Why a block could not be compressed or decompressed: the `exr_result_t` OpenEXRCore
/// reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// A part of the block decompresses to more than it may (`EXR_ERR_OUT_OF_MEMORY`).
    OutOfMemory,
    /// The arguments do not describe the block (`EXR_ERR_INVALID_ARGUMENT`).
    InvalidArgument,
    /// A value is outside what the format allows (`EXR_ERR_ARGUMENT_OUT_OF_RANGE`).
    ArgumentOutOfRange,
    /// The block is of a later version of the format than is supported
    /// (`EXR_ERR_BAD_CHUNK_LEADER`).
    BadChunkLeader,
    /// The block is damaged (`EXR_ERR_CORRUPT_CHUNK`).
    CorruptChunk,
}

impl Error {
    /// OpenEXRCore's number for the error.
    pub const fn code(self) -> i32 {
        match self {
            Error::OutOfMemory => 1,
            Error::InvalidArgument => 3,
            Error::ArgumentOutOfRange => 4,
            Error::BadChunkLeader => 22,
            Error::CorruptChunk => 23,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::OutOfMemory => "a part of the block decompresses to more than it may",
            Error::InvalidArgument => "the arguments do not describe the block",
            Error::ArgumentOutOfRange => "a value is outside what the format allows",
            Error::BadChunkLeader => "the block is of an unsupported version",
            Error::CorruptChunk => "the block is damaged",
        })
    }
}

impl std::error::Error for Error {}

/// The README's examples, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The Guide's examples, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../docs/guide.md")]
struct GuideDoctests;
