//! DWAA and DWAB, DreamWorks Animation's lossy compressions for `.exr` files, a block at a
//! time: a port of OpenEXRCore's (`internal_dwa*.{c,h}` of OpenEXR v3.5.4). [`compress`]
//! compresses a block of an image's lines as a file stores it, and [`decompress`]
//! decompresses one, or reports the [`Error`] OpenEXRCore gives ([`Error::CorruptChunk`] for
//! most damage).
//!
//! A block is its lines one after the other, each with its channels one after the other (in
//! the order of their names, as a file has them), as little-endian halves, floats and 32-bit
//! integers. A channel's name decides what is done with it: the red, green and blue of a
//! layer (`R`, `G` and `B`, or `diffuse.R` and so on) are converted to Y'CbCr together and
//! coded by a lossy DCT, as is a `Y`, `BY` or `RY`; an `A` is kept whole by RLE; anything else
//! is kept whole by ZIP. DWAA's blocks are of 32 lines, DWAB's of 256; they are otherwise the
//! same, but that OpenEXR compresses the AC coefficients of a tile of DWAA with ZIP and every
//! other block's by Huffman coding.
//!
//! The blocks and pixels are those of OpenEXRCore on an x86-64 machine with AVX and F16C (any
//! from about 2011 on), whatever machine this runs on: its arithmetic there is in `simd.rs`,
//! and its lookup tables, which it makes with the C library's `powf` and `logf`, are those
//! glibc makes (`to_linear.bin`, `to_nonlinear.bin`). OpenEXRCore's own blocks differ on
//! other machines. The ZIP of the blocks is libdeflate's, as OpenEXRCore asks for it
//! ([`libdeflate_nrp`]), and the Huffman coding that of PIZ as OpenEXRCore has it (`huf.rs`).
//!
//! Every channel is taken to be sampled at every pixel.

mod huf;
mod lossy;
mod simd;

use crate::compression::{
    DEFAULT_LEVEL, compress_buffer, compress_max_buffer_size, uncompress_buffer,
};
use crate::{Error, PixelType};
use libdeflate_nrp::Level;

/// A table of a half for each half, from its little-endian bytes.
// (Run only at compile time, to build the two tables, so its large array is no burden.)
#[allow(clippy::large_stack_arrays)]
const fn table(bytes: &[u8; 131_072]) -> [u16; 65536] {
    let mut table = [0; 65536];
    let mut i = 0;
    while i < 65536 {
        table[i] = u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]);
        i += 1;
    }
    table
}

/// `exrcore_dwaToLinearTable` and `exrcore_dwaToNonLinearTable`.
static TO_LINEAR: [u16; 65536] = table(include_bytes!("to_linear.bin"));
static TO_NONLINEAR: [u16; 65536] = table(include_bytes!("to_nonlinear.bin"));

fn to_linear(h: u16) -> u16 {
    TO_LINEAR[usize::from(h)]
}

fn to_nonlinear(h: u16) -> u16 {
    TO_NONLINEAR[usize::from(h)]
}

/// A channel of the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Channel<'a> {
    /// The channel's name, which decides how it is compressed.
    pub name: &'a str,
    /// The type of its values.
    pub pixel_type: PixelType,
    /// Whether the file marks the channel's values as perceptually linear (`pLinear`): a
    /// lossy channel's values are then compressed as they are, and not converted to a
    /// perceptual scale first.
    pub linear: bool,
}

impl Channel<'_> {
    fn bytes_per_element(&self) -> usize {
        self.pixel_type.bytes()
    }
}

/// How a block's AC coefficients are compressed, after run-length encoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AcCompression {
    /// Huffman coding, as PIZ codes (`STATIC_HUFFMAN`): OpenEXR's choice for every block
    /// but the tiles of DWAA.
    #[default]
    Huffman,
    /// zlib (`DEFLATE`): OpenEXR's choice for the tiles of DWAA.
    Deflate,
}

// ---------------------------------------------------------------------------------------
// The rules for a channel's compression (`internal_dwa_classifier.h`).

/// How a channel is compressed (`CompressorScheme`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    /// ZIP, as it is: the channels no rule matches.
    Unknown = 0,
    LossyDct = 1,
    Rle = 2,
}

impl Scheme {
    fn from_code(code: u8) -> Option<Scheme> {
        [Scheme::Unknown, Scheme::LossyDct, Scheme::Rle]
            .get(usize::from(code))
            .copied()
    }
}

/// `Classifier`: a suffix of a channel's name, and how such channels are compressed.
#[derive(Clone, Debug)]
struct Rule {
    suffix: Vec<u8>,
    scheme: Scheme,
    pixel_type: PixelType,
    /// Which of red, green and blue such a channel is, for the conversion to Y'CbCr.
    csc_idx: Option<usize>,
    case_insensitive: bool,
}

/// The rules of `sDefaultChannelRules` or `sLegacyChannelRules`.
fn rules(legacy: bool) -> Vec<Rule> {
    let rule = |suffix: &str, scheme: Scheme, pixel_type: PixelType, csc_idx: Option<usize>| Rule {
        suffix: suffix.as_bytes().to_vec(),
        scheme,
        pixel_type,
        csc_idx,
        case_insensitive: legacy,
    };
    let mut out = Vec::new();
    let mut lossy = |suffix: &str, csc_idx: Option<usize>| {
        out.push(rule(suffix, Scheme::LossyDct, PixelType::Half, csc_idx));
        out.push(rule(suffix, Scheme::LossyDct, PixelType::Float, csc_idx));
    };
    if legacy {
        for (suffix, csc_idx) in [
            ("r", Some(0)),
            ("red", Some(0)),
            ("g", Some(1)),
            ("grn", Some(1)),
            ("green", Some(1)),
            ("b", Some(2)),
            ("blu", Some(2)),
            ("blue", Some(2)),
            ("y", None),
            ("by", None),
            ("ry", None),
        ] {
            lossy(suffix, csc_idx);
        }
    } else {
        for (suffix, csc_idx) in [
            ("R", Some(0)),
            ("G", Some(1)),
            ("B", Some(2)),
            ("Y", None),
            ("BY", None),
            ("RY", None),
        ] {
            lossy(suffix, csc_idx);
        }
    }
    let alpha = if legacy { "a" } else { "A" };
    for pixel_type in [PixelType::Uint, PixelType::Half, PixelType::Float] {
        out.push(rule(alpha, Scheme::Rle, pixel_type, None));
    }
    out
}

/// `Classifier_find_suffix`: what follows the last `.` of the name, or all of it.
fn find_suffix(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&byte| byte == b'.') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

/// `Classifier_match`.
fn rule_matches(rule: &Rule, suffix: &[u8], pixel_type: PixelType) -> bool {
    if rule.pixel_type != pixel_type {
        return false;
    }
    if rule.case_insensitive {
        return suffix.eq_ignore_ascii_case(&rule.suffix);
    }
    suffix == rule.suffix.as_slice()
}

/// `Classifier_size`.
fn rule_size(rule: &Rule) -> usize {
    rule.suffix.len() + 1 + 2
}

/// `Classifier_write`.
fn write_rule(rule: &Rule, out: &mut Vec<u8>) {
    out.extend_from_slice(&rule.suffix);
    out.push(0);
    let csc = rule.csc_idx.map_or(0, |i| i as u8 + 1);
    out.push((csc & 15) << 4 | (rule.scheme as u8 & 3) << 2 | u8::from(rule.case_insensitive));
    out.push(rule.pixel_type as u8);
}

/// `Classifier_read`: a rule from the start of `data`, and how many bytes it took.
fn read_rule(data: &[u8]) -> Result<(Rule, usize), Error> {
    let size = data.len();
    if size <= 3 {
        return Err(Error::CorruptChunk);
    }
    // (The suffix is no more than 128 bytes and its nil.)
    let mut len = 0usize;
    while len < 128 + 1 {
        if len > size - 3 {
            return Err(Error::CorruptChunk);
        }
        if data[len] == 0 {
            break;
        }
        len += 1;
    }
    if len == 128 + 1 {
        return Err(Error::CorruptChunk);
    }
    let suffix = data[..len].to_vec();
    len += 1;
    if size < len + 2 {
        return Err(Error::CorruptChunk);
    }
    let value = data[len];
    let pixel_type = data[len + 1];

    let csc_idx = match value >> 4 {
        0 => None,
        n @ 1..=3 => Some(usize::from(n) - 1),
        _ => return Err(Error::CorruptChunk),
    };
    let scheme = Scheme::from_code((value >> 2) & 3).ok_or(Error::CorruptChunk)?;
    let case_insensitive = value & 1 != 0;
    let pixel_type = PixelType::from_code(i32::from(pixel_type)).ok_or(Error::CorruptChunk)?;
    Ok((
        Rule {
            suffix,
            scheme,
            pixel_type,
            csc_idx,
            case_insensitive,
        },
        len + 2,
    ))
}

// ---------------------------------------------------------------------------------------
// The compressor (`internal_dwa_compressor.h`).

/// What `DwaCompressor_classifyChannels` and `DwaCompressor_initializeBuffers` find of the
/// channels and the buffers they need.
struct Layout {
    /// Each channel's compression.
    compression: Vec<Scheme>,
    /// The channels converted to Y'CbCr together, red, green, blue.
    csc_sets: Vec<[usize; 3]>,
    out_buffer_size: usize,
    packed_ac_size: usize,
    packed_dc_size: usize,
    rle_buffer_size: usize,
    planar_size: [usize; 3],
    /// The bytes of the channels kept whole by ZIP.
    unknown_size: usize,
    /// Where each channel's data starts in its scheme's planar buffer.
    planar_at: Vec<usize>,
}

fn classify(channels: &[Channel], rules: &[Rule]) -> (Vec<Scheme>, Vec<[usize; 3]>) {
    let mut compression = vec![Scheme::Unknown; channels.len()];
    // (prefix, the channels of each of its colours)
    let mut prefix_map: Vec<(&[u8], [Option<usize>; 3])> = Vec::new();
    for (c, channel) in channels.iter().enumerate() {
        let name = channel.name.as_bytes();
        let suffix = find_suffix(name);
        let prefix = &name[..name.len() - suffix.len()];
        let at = if let Some(at) = prefix_map.iter().position(|(p, _)| *p == prefix) {
            at
        } else {
            prefix_map.push((prefix, [None; 3]));
            prefix_map.len() - 1
        };
        for rule in rules {
            if rule_matches(rule, suffix, channel.pixel_type) {
                compression[c] = rule.scheme;
                if let Some(idx) = rule.csc_idx {
                    prefix_map[at].1[idx] = Some(c);
                }
            }
        }
    }
    let csc_sets = prefix_map
        .iter()
        .filter_map(|&(_, [r, g, b])| Some([r?, g?, b?]))
        .collect();
    (compression, csc_sets)
}

/// `ceilf((float) n / 8.0f)`.
fn blocks_of(n: usize) -> usize {
    ((n as f32) / 8.0).ceil() as usize
}

fn layout(channels: &[Channel], rules: &[Rule], width: usize, lines: usize) -> Layout {
    let (compression, csc_sets) = classify(channels, rules);
    let max_ac = blocks_of(lines) * blocks_of(width) * 63 * 2;
    let max_dc = blocks_of(lines) * blocks_of(width) * 2;
    let pixel_count = lines * width;

    let mut out_buffer_size: usize = rules.iter().map(rule_size).sum();
    let mut num_lossy = 0usize;
    let mut rle_buffer_size = 0usize;
    let mut unknown_buffer_size = 0usize;
    let mut planar_size = [0usize; 3];
    for (c, channel) in channels.iter().enumerate() {
        let bpe = channel.bytes_per_element();
        match compression[c] {
            Scheme::LossyDct => {
                out_buffer_size += (2 * max_ac + 65536).max(compress_max_buffer_size(max_ac));
                num_lossy += 1;
            }
            Scheme::Rle => {
                rle_buffer_size += 2 * pixel_count * bpe;
                planar_size[Scheme::Rle as usize] += 2 * pixel_count * bpe;
            }
            Scheme::Unknown => {
                unknown_buffer_size += pixel_count * bpe;
                planar_size[Scheme::Unknown as usize] += pixel_count * bpe;
            }
        }
    }
    out_buffer_size += compress_max_buffer_size(rle_buffer_size);
    out_buffer_size += compress_max_buffer_size(unknown_buffer_size);
    out_buffer_size += compress_max_buffer_size(max_dc * num_lossy);
    out_buffer_size += Counters::SIZE;
    if planar_size[Scheme::Unknown as usize] > 0 {
        planar_size[Scheme::Unknown as usize] =
            compress_max_buffer_size(planar_size[Scheme::Unknown as usize]);
    }

    // DwaCompressor_setupChannelData
    let mut next = [0usize; 3];
    let mut planar_at = Vec::with_capacity(channels.len());
    for (c, channel) in channels.iter().enumerate() {
        let scheme = compression[c] as usize;
        planar_at.push(next[scheme]);
        if compression[c] != Scheme::LossyDct {
            next[scheme] += width * lines * channel.bytes_per_element();
        }
    }

    Layout {
        compression,
        csc_sets,
        out_buffer_size,
        packed_ac_size: max_ac * num_lossy,
        packed_dc_size: max_dc * num_lossy,
        rle_buffer_size,
        planar_size,
        unknown_size: unknown_buffer_size,
        planar_at,
    }
}

/// The counters at the start of a block, in the order it stores them, as little-endian
/// 64-bit values.
#[derive(Clone, Copy, Default)]
struct Counters {
    version: u64,
    unknown_uncompressed_size: u64,
    unknown_compressed_size: u64,
    ac_compressed_size: u64,
    dc_compressed_size: u64,
    rle_compressed_size: u64,
    rle_uncompressed_size: u64,
    rle_raw_size: u64,
    ac_uncompressed_count: u64,
    dc_uncompressed_count: u64,
    /// How the AC coefficients are compressed: [`STATIC_HUFFMAN`] or [`DEFLATE`].
    ac_compression: u64,
}

/// How the AC coefficients are compressed.
const STATIC_HUFFMAN: u64 = 0;
const DEFLATE: u64 = 1;

impl Counters {
    /// The bytes the counters take.
    const SIZE: usize = 11 * 8;

    fn to_array(self) -> [u64; 11] {
        [
            self.version,
            self.unknown_uncompressed_size,
            self.unknown_compressed_size,
            self.ac_compressed_size,
            self.dc_compressed_size,
            self.rle_compressed_size,
            self.rle_uncompressed_size,
            self.rle_raw_size,
            self.ac_uncompressed_count,
            self.dc_uncompressed_count,
            self.ac_compression,
        ]
    }

    fn read(header: &[u8; Self::SIZE]) -> Self {
        let [
            version,
            unknown_uncompressed_size,
            unknown_compressed_size,
            ac_compressed_size,
            dc_compressed_size,
            rle_compressed_size,
            rle_uncompressed_size,
            rle_raw_size,
            ac_uncompressed_count,
            dc_uncompressed_count,
            ac_compression,
        ] = {
            let (words, _) = header.as_chunks::<8>();
            core::array::from_fn(|i| u64::from_le_bytes(words[i]))
        };
        Self {
            version,
            unknown_uncompressed_size,
            unknown_compressed_size,
            ac_compressed_size,
            dc_compressed_size,
            rle_compressed_size,
            rle_uncompressed_size,
            rle_raw_size,
            ac_uncompressed_count,
            dc_uncompressed_count,
            ac_compression,
        }
    }

    fn write(self, out: &mut [u8]) {
        for (bytes, value) in out.as_chunks_mut::<8>().0.iter_mut().zip(self.to_array()) {
            *bytes = value.to_le_bytes();
        }
    }

    /// The checks OpenEXRCore makes of the counters of a block of `block_size` bytes that
    /// decompresses to `uncompressed_size`.
    fn check(&self, block_size: u64, uncompressed_size: u64) -> Result<(), Error> {
        let compressed_size = self
            .unknown_compressed_size
            .wrapping_add(self.ac_compressed_size)
            .wrapping_add(self.dc_compressed_size)
            .wrapping_add(self.rle_compressed_size);
        // Both the sum and individual sizes are checked in case of overflow.
        if block_size < (Self::SIZE as u64).wrapping_add(compressed_size)
            || block_size < self.unknown_compressed_size
            || block_size < self.ac_compressed_size
            || block_size < self.dc_compressed_size
            || block_size < self.rle_compressed_size
        {
            return Err(Error::CorruptChunk);
        }
        if self.unknown_uncompressed_size > uncompressed_size
            || self.rle_raw_size > uncompressed_size
            || self
                .unknown_uncompressed_size
                .wrapping_add(self.rle_raw_size)
                > uncompressed_size
        {
            return Err(Error::CorruptChunk);
        }
        // (All but the version and the AC compression must be below 2^63.)
        if self.to_array()[1..10].iter().any(|&size| (size as i64) < 0) {
            return Err(Error::CorruptChunk);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// RLE (`internal_rle.c`) and the byte order of ZIP (`internal_zip.c`).

const MIN_RUN_LENGTH: usize = 3;
const MAX_RUN_LENGTH: usize = 127;

/// `internal_rle_compress`, into no more than about `out_bytes`.
fn rle_compress(src: &[u8], out_bytes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let end = src.len();
    let mut runs = 0usize;
    let mut rune = 1usize;
    while runs < end {
        let mut curcount = 0usize;
        while rune < end && src[runs] == src[rune] && curcount < MAX_RUN_LENGTH {
            rune += 1;
            curcount += 1;
        }
        if curcount >= MIN_RUN_LENGTH - 1 {
            out.push(curcount as u8);
            out.push(src[runs]);
            runs = rune;
        } else {
            // incompressible
            curcount += 1;
            while rune < end
                && ((rune + 1 >= end || src[rune] != src[rune + 1])
                    || (rune + 2 >= end || src[rune + 1] != src[rune + 2]))
                && curcount < MAX_RUN_LENGTH
            {
                curcount += 1;
                rune += 1;
            }
            out.push((-(curcount as i32)) as u8);
            out.extend_from_slice(&src[runs..rune]);
            runs = rune;
        }
        rune += 1;
        if out.len() >= out_bytes {
            break;
        }
    }
    out
}

/// `internal_rle_decompress`: the bytes made, or 0 for a stream that does not fit.
fn rle_decompress(out: &mut [u8], src: &[u8]) -> usize {
    let packsz = src.len();
    let outsz = out.len();
    let mut at = 0usize;
    let mut unpackbytes = 0usize;
    let mut outbytes = 0usize;
    while unpackbytes < packsz {
        let byte = src[at] as i8;
        if byte < 0 {
            let count = (-(i32::from(byte))) as usize;
            at += 1;
            unpackbytes += 1;
            if unpackbytes + count > packsz {
                return 0;
            }
            if outbytes + count > outsz {
                return 0;
            }
            out[outbytes..outbytes + count].copy_from_slice(&src[at..at + count]);
            at += count;
            unpackbytes += count;
            outbytes += count;
        } else {
            let count = byte as usize;
            at += 1;
            if unpackbytes + 2 > packsz {
                return 0;
            }
            unpackbytes += 2;
            let count = count + 1;
            if outbytes + count > outsz {
                return 0;
            }
            let value = src[at];
            out[outbytes..outbytes + count].fill(value);
            outbytes += count;
            at += 1;
        }
    }
    outbytes
}

/// `internal_zip_deconstruct_bytes`: the even bytes then the odd, as differences.
fn zip_deconstruct_bytes(source: &[u8]) -> Vec<u8> {
    let count = source.len();
    let mut scratch = vec![0u8; count];
    let half = count.div_ceil(2);
    for (i, &byte) in source.iter().enumerate() {
        if i % 2 == 0 {
            scratch[i / 2] = byte;
        } else {
            scratch[half + i / 2] = byte;
        }
    }
    let mut p = i32::from(*scratch.first().unwrap_or(&0));
    for t in scratch.iter_mut().skip(1) {
        let d = i32::from(*t) - p + (128 + 256);
        p = i32::from(*t);
        *t = d as u8;
    }
    scratch
}

/// `internal_zip_reconstruct_bytes`.
fn zip_reconstruct_bytes(source: &mut [u8]) -> Vec<u8> {
    for i in 1..source.len() {
        source[i] = (i32::from(source[i - 1]) + i32::from(source[i]) - 128) as u8;
    }
    let count = source.len();
    let half = count.div_ceil(2);
    (0..count)
        .map(|i| {
            if i % 2 == 0 {
                source[i / 2]
            } else {
                source[half + i / 2]
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------

/// Where a block's channels lie: each line holds every channel's values for that line, one
/// channel after another.
struct LineLayout {
    /// The bytes of a line.
    line_size: usize,
    /// Where each channel starts within a line.
    channel_at: Vec<usize>,
    lines: usize,
}

impl LineLayout {
    fn new(channels: &[Channel], width: usize, lines: usize) -> Self {
        let mut channel_at = Vec::with_capacity(channels.len());
        let mut line_size = 0;
        for channel in channels {
            channel_at.push(line_size);
            line_size += width * channel.bytes_per_element();
        }
        Self {
            line_size,
            channel_at,
            lines,
        }
    }

    /// Where each line of channel `c` starts in the block.
    fn rows(&self, c: usize) -> Vec<usize> {
        (0..self.lines)
            .map(|y| y * self.line_size + self.channel_at[c])
            .collect()
    }
}

/// Compresses the block `raw` of `width` pixels and `lines` lines with DWAA or DWAB
/// (`DwaCompressor_compress`).
///
/// `raw` holds the lines one after the other, each with its channels one after the other, in
/// the order of `channels`, as little-endian values. `level` is the file's
/// `dwaCompressionLevel` (OpenEXR's default is 45): higher is smaller and lossier. A block
/// that does not come out smaller is returned as it is, as OpenEXR stores it.
///
/// # Errors
///
/// [`Error::InvalidArgument`] when `raw` is not the length the channels, width and lines
/// make.
pub fn compress(
    raw: &[u8],
    channels: &[Channel],
    width: usize,
    lines: usize,
    level: f32,
    ac_compression: AcCompression,
) -> Result<Vec<u8>, Error> {
    let rows = LineLayout::new(channels, width, lines);
    if rows.line_size.checked_mul(lines) != Some(raw.len()) {
        return Err(Error::InvalidArgument);
    }
    let rules = rules(false);
    let layout = layout(channels, &rules, width, lines);
    let encoder = Encoder {
        raw,
        channels,
        width,
        lines,
        rows: &rows,
        layout: &layout,
        quant_base_error: level / 100_000.0,
    };

    let mut counters = Counters {
        version: 2,
        ac_compression: match ac_compression {
            AcCompression::Huffman => STATIC_HUFFMAN,
            AcCompression::Deflate => DEFLATE,
        },
        ..Counters::default()
    };
    // The counters, written last, then the rules relevant to the channels.
    let mut out = vec![0u8; Counters::SIZE];
    write_rules(&mut out, &rules, channels);

    let mut coefficients = lossy::Coefficients::default();
    let processed = encoder.encode_csc_sets(&mut coefficients);
    let planar = encoder.split_channels(&processed, &mut coefficients, &mut counters);
    let lossy::Coefficients {
        ac: packed_ac,
        dc: packed_dc,
    } = coefficients;
    counters.ac_uncompressed_count = packed_ac.len() as u64;
    counters.dc_uncompressed_count = packed_dc.len() as u64;

    // UNKNOWN data first, then the AC coefficients, the DC values and the RLE data.
    if counters.unknown_uncompressed_size > 0 {
        let size = counters.unknown_uncompressed_size as usize;
        let packed = compress_buffer(Level::BEST, &planar.unknown[..size]);
        counters.unknown_compressed_size = packed.len() as u64;
        out.extend_from_slice(&packed);
    }
    if !packed_ac.is_empty() {
        let packed = match ac_compression {
            AcCompression::Deflate => {
                let bytes: Vec<u8> = packed_ac.iter().flat_map(|v| v.to_le_bytes()).collect();
                compress_buffer(Level::BEST, &bytes)
            }
            AcCompression::Huffman => {
                match huf::compress(&packed_ac, layout.out_buffer_size - out.len()) {
                    Ok(packed) => packed,
                    // (Stored as it is, the block too large for the coder.)
                    Err(_) => return Ok(raw.to_vec()),
                }
            }
        };
        counters.ac_compressed_size = packed.len() as u64;
        out.extend_from_slice(&packed);
    }
    if !packed_dc.is_empty() {
        let bytes: Vec<u8> = packed_dc.iter().flat_map(|v| v.to_le_bytes()).collect();
        let packed = compress_buffer(DEFAULT_LEVEL, &zip_deconstruct_bytes(&bytes));
        counters.dc_compressed_size = packed.len() as u64;
        out.extend_from_slice(&packed);
    }
    if counters.rle_raw_size > 0 {
        let size = counters.rle_raw_size as usize;
        let rle = rle_compress(&planar.rle[..size], layout.rle_buffer_size);
        counters.rle_uncompressed_size = rle.len() as u64;
        let packed = compress_buffer(Level::BEST, &rle);
        counters.rle_compressed_size = packed.len() as u64;
        out.extend_from_slice(&packed);
    }

    counters.write(&mut out[..Counters::SIZE]);
    if out.len() >= raw.len() {
        return Ok(raw.to_vec());
    }
    Ok(out)
}

/// Writes the rules that match one of `channels` or more, after their size in bytes.
fn write_rules(out: &mut Vec<u8>, rules: &[Rule], channels: &[Channel]) {
    let rules_at = out.len();
    out.extend_from_slice(&[0, 0]);
    for rule in rules {
        let matches = |channel: &Channel| {
            let suffix = find_suffix(channel.name.as_bytes());
            rule_matches(rule, suffix, channel.pixel_type)
        };
        if channels.iter().any(matches) {
            write_rule(rule, out);
        }
    }
    let rule_size = (out.len() - rules_at) as u16;
    out[rules_at..rules_at + 2].copy_from_slice(&rule_size.to_le_bytes());
}

/// The channels' data, rearranged for the schemes that are not lossy.
struct Planar {
    /// The channels kept whole (UNKNOWN), one after another.
    unknown: Vec<u8>,
    /// The channels run-length coded, each as the planes of its values' bytes.
    rle: Vec<u8>,
}

/// A block being compressed, and what is known of its channels.
struct Encoder<'a> {
    raw: &'a [u8],
    channels: &'a [Channel<'a>],
    width: usize,
    lines: usize,
    rows: &'a LineLayout,
    layout: &'a Layout,
    quant_base_error: f32,
}

impl Encoder<'_> {
    /// Codes each set of red, green and blue channels into `coefficients` as Y'CbCr, and
    /// returns which channels those are.
    fn encode_csc_sets(&self, coefficients: &mut lossy::Coefficients) -> Vec<bool> {
        let mut processed = vec![false; self.channels.len()];
        for set in &self.layout.csc_sets {
            let sources: Vec<lossy::Source> = set
                .iter()
                .map(|&c| lossy::Source {
                    rows: self.rows.rows(c),
                    pixel_type: self.channels[c].pixel_type,
                })
                .collect();
            let (width, lines) = (self.width, self.lines);
            let base_error = self.quant_base_error;
            lossy::encode(
                self.raw,
                &sources,
                base_error,
                true,
                width,
                lines,
                coefficients,
            );
            for &c in set {
                processed[c] = true;
            }
        }
        processed
    }

    /// Codes each channel not yet `processed` by its scheme: lossy ones into `coefficients`,
    /// the others into the planar buffers returned, counting their bytes in `counters`.
    fn split_channels(
        &self,
        processed: &[bool],
        coefficients: &mut lossy::Coefficients,
        counters: &mut Counters,
    ) -> Planar {
        let (raw, width, lines) = (self.raw, self.width, self.lines);
        // (Only the bytes the channels fill: OpenEXRCore's buffers are larger.)
        let mut planar = Planar {
            unknown: vec![0u8; self.layout.unknown_size],
            rle: vec![0u8; self.layout.planar_size[Scheme::Rle as usize]],
        };
        for (c, channel) in self.channels.iter().enumerate() {
            if processed[c] {
                continue;
            }
            let bpe = channel.bytes_per_element();
            let row_bytes = width * bpe;
            let base = self.layout.planar_at[c];
            match self.layout.compression[c] {
                Scheme::LossyDct => {
                    let source = lossy::Source {
                        rows: self.rows.rows(c),
                        pixel_type: channel.pixel_type,
                    };
                    let base_error = self.quant_base_error;
                    let nonlinear = !channel.linear;
                    lossy::encode(
                        raw,
                        &[source],
                        base_error,
                        nonlinear,
                        width,
                        lines,
                        coefficients,
                    );
                }
                Scheme::Rle => {
                    // The first bytes of each value together, then the second, and so on.
                    let plane = width * lines;
                    let mut end = [0usize; 4];
                    for row in self.rows.rows(c) {
                        for x in 0..width {
                            for (byte, e) in end.iter_mut().enumerate().take(bpe) {
                                planar.rle[base + byte * plane + *e] = raw[row + x * bpe + byte];
                                *e += 1;
                            }
                        }
                        counters.rle_raw_size += row_bytes as u64;
                    }
                }
                Scheme::Unknown => {
                    for (y, row) in self.rows.rows(c).into_iter().enumerate() {
                        planar.unknown[base + y * row_bytes..base + (y + 1) * row_bytes]
                            .copy_from_slice(&raw[row..row + row_bytes]);
                    }
                    counters.unknown_uncompressed_size += (width * lines * bpe) as u64;
                }
            }
        }
        planar
    }
}

/// Decompresses the DWAA or DWAB block `data` of `width` pixels and `lines` lines into `out`
/// (`DwaCompressor_uncompress`).
///
/// `out` is the block's uncompressed size, and receives its lines as [`compress`] takes them.
///
/// # Errors
///
/// For a block that OpenEXRCore refuses, the [`Error`] it reports, most often
/// [`Error::CorruptChunk`].
pub fn decompress(
    data: &[u8],
    channels: &[Channel],
    width: usize,
    lines: usize,
    out: &mut [u8],
) -> Result<(), Error> {
    let Some(header) = data.first_chunk::<{ Counters::SIZE }>() else {
        return Err(Error::CorruptChunk);
    };
    out.fill(0);
    let counters = Counters::read(header);
    counters.check(data.len() as u64, out.len() as u64)?;

    let mut data_at = Counters::SIZE;
    let rules = if counters.version < 2 {
        rules(true)
    } else {
        let (rules, size) = read_rules(&data[data_at..])?;
        data_at += size;
        rules
    };
    let layout = layout(channels, &rules, width, lines);
    let parts = Parts::locate(data, data_at, &counters)?;
    if counters.version > 2 {
        return Err(Error::BadChunkLeader);
    }
    let required = |scheme: Scheme| -> u64 {
        let bytes = |(_, channel): (usize, &Channel)| width * lines * channel.bytes_per_element();
        let of_scheme = |(c, _): &(usize, &Channel)| layout.compression[*c] == scheme;
        channels
            .iter()
            .enumerate()
            .filter(of_scheme)
            .map(bytes)
            .sum::<usize>() as u64
    };
    validate_size(
        required(Scheme::Unknown),
        counters.unknown_compressed_size,
        counters.unknown_uncompressed_size,
        0,
    )?;
    validate_size(
        required(Scheme::Rle),
        counters.rle_compressed_size,
        counters.rle_raw_size,
        counters.rle_uncompressed_size,
    )?;

    let unpacked = Unpacked {
        unknown: parts.unpack_unknown(&counters, &layout)?,
        ac: parts.unpack_ac(&counters, &layout)?,
        dc: parts.unpack_dc(&counters, &layout)?,
        rle: parts.unpack_rle(&counters, &layout)?,
    };
    let decoder = Decoding {
        channels,
        width,
        lines,
        rows: LineLayout::new(channels, width, lines),
        layout: &layout,
    };
    decoder.decode(out, &unpacked, &counters)
}

/// `DwaCompressor_readChannelRules`: the rules at the start of `data`, and the bytes they
/// take.
fn read_rules(data: &[u8]) -> Result<(Vec<Rule>, usize), Error> {
    if data.len() <= 2 {
        return Err(Error::CorruptChunk);
    }
    let rule_size = usize::from(u16::from_le_bytes([data[0], data[1]]));
    if rule_size < 2 || data.len() < rule_size {
        return Err(Error::CorruptChunk);
    }
    let mut rule_data = &data[2..rule_size];
    let mut rules = Vec::new();
    while !rule_data.is_empty() {
        let (rule, taken) = read_rule(rule_data)?;
        rules.push(rule);
        rule_data = &rule_data[taken..];
    }
    Ok((rules, rule_size))
}

/// `validate_size`: a scheme's sizes agree with the bytes its channels need, `required`.
fn validate_size(
    required: u64,
    compressed: u64,
    uncompressed: u64,
    extra: u64,
) -> Result<(), Error> {
    let agree = if required > 0 {
        uncompressed >= required && compressed > 0
    } else {
        uncompressed == 0 && compressed == 0 && extra == 0
    };
    if agree {
        Ok(())
    } else {
        Err(Error::CorruptChunk)
    }
}

/// The compressed parts of a block, in the order it stores them.
struct Parts<'a> {
    unknown: &'a [u8],
    ac: &'a [u8],
    dc: &'a [u8],
    rle: &'a [u8],
}

impl<'a> Parts<'a> {
    /// The parts of `data` from `at`, as its counters size them.
    fn locate(data: &'a [u8], at: usize, counters: &Counters) -> Result<Self, Error> {
        let block_size = data.len() as u64;
        let unknown_at = at as u64;
        let ac_at = unknown_at + counters.unknown_compressed_size;
        let dc_at = ac_at + counters.ac_compressed_size;
        let rle_at = dc_at + counters.dc_compressed_size;
        let rle_end = rle_at + counters.rle_compressed_size;
        if ac_at > block_size || dc_at > block_size || rle_at > block_size || rle_end > block_size {
            return Err(Error::CorruptChunk);
        }
        let part = |from: u64, to: u64| &data[from as usize..to as usize];
        Ok(Self {
            unknown: part(unknown_at, ac_at),
            ac: part(ac_at, dc_at),
            dc: part(dc_at, rle_at),
            rle: part(rle_at, rle_end),
        })
    }

    /// The channels kept whole, after ZIP.
    fn unpack_unknown(&self, counters: &Counters, layout: &Layout) -> Result<Vec<u8>, Error> {
        let size = counters.unknown_uncompressed_size;
        // (Only the bytes the stream holds or the channels read: OpenEXRCore's buffer is
        // larger.)
        if size > layout.planar_size[Scheme::Unknown as usize] as u64 {
            return Err(Error::CorruptChunk);
        }
        let mut planar = vec![0u8; layout.unknown_size.max(size as usize)];
        if counters.unknown_compressed_size > 0 {
            let made = uncompress_buffer(self.unknown, &mut planar[..size as usize])
                .map_err(|_| Error::CorruptChunk)?;
            if made as u64 != size {
                return Err(Error::CorruptChunk);
            }
        }
        Ok(planar)
    }

    /// The AC coefficients, after Huffman coding or ZIP.
    fn unpack_ac(&self, counters: &Counters, layout: &Layout) -> Result<Vec<u16>, Error> {
        let mut packed = vec![0u16; layout.packed_ac_size / 2];
        if counters.ac_compressed_size == 0 {
            return Ok(packed);
        }
        let count = counters.ac_uncompressed_count;
        if layout.packed_ac_size == 0 || count * 2 > layout.packed_ac_size as u64 {
            return Err(Error::CorruptChunk);
        }
        let count = count as usize;
        match counters.ac_compression {
            STATIC_HUFFMAN => huf::decompress(self.ac, &mut packed[..count])?,
            DEFLATE => {
                let mut bytes = vec![0u8; count * 2];
                let made = uncompress_buffer(self.ac, &mut bytes)?;
                if made != count * 2 {
                    return Err(Error::CorruptChunk);
                }
                for (value, pair) in packed.iter_mut().zip(bytes.as_chunks::<2>().0) {
                    *value = u16::from_le_bytes(*pair);
                }
            }
            _ => return Err(Error::CorruptChunk),
        }
        Ok(packed)
    }

    /// The DC values, after ZIP of their reordered bytes.
    fn unpack_dc(&self, counters: &Counters, layout: &Layout) -> Result<Vec<u16>, Error> {
        let mut packed = vec![0u16; layout.packed_dc_size / 2];
        if counters.dc_compressed_size == 0 {
            // If the compressed size is 0, then the uncompressed size must also be zero.
            return if counters.dc_uncompressed_count == 0 {
                Ok(packed)
            } else {
                Err(Error::CorruptChunk)
            };
        }
        let size = counters.dc_uncompressed_count * 2;
        if size > layout.packed_dc_size as u64 {
            return Err(Error::CorruptChunk);
        }
        let mut scratch = vec![0u8; size as usize];
        match uncompress_buffer(self.dc, &mut scratch) {
            Ok(made) if made as u64 == size => {}
            _ => return Err(Error::CorruptChunk),
        }
        let bytes = zip_reconstruct_bytes(&mut scratch);
        for (value, pair) in packed.iter_mut().zip(bytes.as_chunks::<2>().0) {
            *value = u16::from_le_bytes(*pair);
        }
        Ok(packed)
    }

    /// The run-length coded channels, after ZIP and the run-length decoding.
    fn unpack_rle(&self, counters: &Counters, layout: &Layout) -> Result<Vec<u8>, Error> {
        let mut planar = vec![0u8; layout.planar_size[Scheme::Rle as usize]];
        let raw_size = counters.rle_raw_size;
        if raw_size == 0 {
            return Ok(planar);
        }
        let size = counters.rle_uncompressed_size;
        if size > layout.rle_buffer_size as u64 || raw_size > planar.len() as u64 {
            return Err(Error::CorruptChunk);
        }
        let mut rle = vec![0u8; size as usize];
        match uncompress_buffer(self.rle, &mut rle) {
            Ok(made) if made as u64 == size => {}
            _ => return Err(Error::CorruptChunk),
        }
        if rle_decompress(&mut planar[..raw_size as usize], &rle) as u64 != raw_size {
            return Err(Error::CorruptChunk);
        }
        Ok(planar)
    }
}

/// A block's parts, decompressed.
struct Unpacked {
    unknown: Vec<u8>,
    ac: Vec<u16>,
    dc: Vec<u16>,
    rle: Vec<u8>,
}

/// A block being decompressed, and what is known of its channels.
struct Decoding<'a> {
    channels: &'a [Channel<'a>],
    width: usize,
    lines: usize,
    rows: LineLayout,
    layout: &'a Layout,
}

impl Decoding<'_> {
    /// Decodes every channel into `out`: the sets of red, green and blue first, from Y'CbCr,
    /// then the others in order, each by its scheme.
    fn decode(
        &self,
        out: &mut [u8],
        unpacked: &Unpacked,
        counters: &Counters,
    ) -> Result<(), Error> {
        let (width, lines, layout) = (self.width, self.lines, self.layout);
        let mut decoder = lossy::Decoder::new(self.channels.len());
        let mut packed = lossy::Packed {
            ac: &unpacked.ac,
            ac_at: 0,
            ac_left: counters.ac_uncompressed_count,
            dc: &unpacked.dc,
            dc_at: 0,
            dc_left: counters.dc_uncompressed_count,
        };
        let mut processed = vec![false; self.channels.len()];

        for set in &layout.csc_sets {
            if set
                .iter()
                .any(|&c| layout.compression[c] != Scheme::LossyDct)
            {
                return Err(Error::CorruptChunk);
            }
            let targets: Vec<lossy::Target> = set
                .iter()
                .map(|&c| lossy::Target {
                    channel: c,
                    rows: self.rows.rows(c),
                    pixel_type: self.channels[c].pixel_type,
                })
                .collect();
            let result = decoder.decode(out, &targets, &mut packed, true, width, lines);
            for &c in set {
                processed[c] = true;
            }
            result?;
        }

        for (c, channel) in self.channels.iter().enumerate() {
            if processed[c] || width == 0 || lines == 0 {
                continue;
            }
            let bpe = channel.bytes_per_element();
            match layout.compression[c] {
                Scheme::LossyDct => {
                    let target = lossy::Target {
                        channel: c,
                        rows: self.rows.rows(c),
                        pixel_type: channel.pixel_type,
                    };
                    let nonlinear = !channel.linear;
                    decoder.decode(out, &[target], &mut packed, nonlinear, width, lines)?;
                }
                Scheme::Rle => {
                    let plane = width * lines;
                    let base = layout.planar_at[c];
                    let mut end = [0usize; 4];
                    for row in self.rows.rows(c) {
                        for x in 0..width {
                            for (byte, e) in end.iter_mut().enumerate().take(bpe) {
                                out[row + x * bpe + byte] = unpacked.rle[base + byte * plane + *e];
                                *e += 1;
                            }
                        }
                    }
                }
                Scheme::Unknown => {
                    let row_bytes = width * bpe;
                    let mut at = layout.planar_at[c];
                    for row in self.rows.rows(c) {
                        // (A sanity check that the data lies within range.)
                        let Some(source) = unpacked.unknown.get(at..at + row_bytes) else {
                            return Err(Error::CorruptChunk);
                        };
                        out[row..row + row_bytes].copy_from_slice(source);
                        at += row_bytes;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(
        width: usize,
        lines: usize,
        channels: &[Channel],
        value: impl Fn(usize, usize, usize) -> u32,
    ) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..lines {
            for (c, channel) in channels.iter().enumerate() {
                for x in 0..width {
                    let v = value(c, x, y);
                    if channel.pixel_type == PixelType::Half {
                        raw.extend_from_slice(&(v as u16).to_le_bytes());
                    } else {
                        raw.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
        }
        raw
    }

    #[test]
    fn what_is_kept_whole_comes_back_whole() {
        let channels = [
            Channel {
                name: "A",
                pixel_type: PixelType::Half,
                linear: false,
            },
            Channel {
                name: "id",
                pixel_type: PixelType::Uint,
                linear: false,
            },
        ];
        let raw = block(37, 32, &channels, |c, x, y| {
            if c == 0 {
                0x3c00
            } else {
                (x / 5 + y / 3) as u32
            }
        });
        let packed = compress(&raw, &channels, 37, 32, 45.0, AcCompression::Huffman).unwrap();
        assert!(packed.len() < raw.len() / 4);
        let mut out = vec![0u8; raw.len()];
        decompress(&packed, &channels, 37, 32, &mut out).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn colour_comes_back_near() {
        let channels: Vec<Channel> = ["B", "G", "R"]
            .iter()
            .map(|name| Channel {
                name,
                pixel_type: PixelType::Half,
                linear: false,
            })
            .collect();
        let half = |f: f32| u32::from(simd::float_to_half(f));
        let raw = block(64, 40, &channels, |c, x, y| {
            half(0.25 + (x + 2 * y + 9 * c) as f32 / 200.0)
        });
        for ac in [AcCompression::Huffman, AcCompression::Deflate] {
            let packed = compress(&raw, &channels, 64, 40, 45.0, ac).unwrap();
            assert!(packed.len() < raw.len() / 3);
            let mut out = vec![0u8; raw.len()];
            decompress(&packed, &channels, 64, 40, &mut out).unwrap();
            for (a, b) in raw
                .as_chunks::<2>()
                .0
                .iter()
                .zip(out.as_chunks::<2>().0.iter())
            {
                let (a, b) = (
                    simd::half_to_float(u16::from_le_bytes([a[0], a[1]])),
                    simd::half_to_float(u16::from_le_bytes([b[0], b[1]])),
                );
                assert!((a - b).abs() < 0.01, "{a} {b}");
            }
        }
    }

    #[test]
    fn damage_is_refused() {
        let channels = [Channel {
            name: "Y",
            pixel_type: PixelType::Float,
            linear: false,
        }];
        let raw = block(16, 16, &channels, |_, x, y| {
            (0.5 + (x * y) as f32 / 512.0).to_bits()
        });
        let mut packed = compress(&raw, &channels, 16, 16, 45.0, AcCompression::Huffman).unwrap();
        assert_eq!(
            compress(&raw[1..], &channels, 16, 16, 45.0, AcCompression::Huffman),
            Err(Error::InvalidArgument)
        );
        let mut out = vec![0u8; raw.len()];
        assert_eq!(
            decompress(&packed[..40], &channels, 16, 16, &mut out),
            Err(Error::CorruptChunk)
        );
        packed[0] = 3;
        assert_eq!(
            decompress(&packed, &channels, 16, 16, &mut out),
            Err(Error::BadChunkLeader)
        );
    }
}
