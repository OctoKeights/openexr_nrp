//! DWAA and DWAB throughput on a synthetic 1024 by 512 image of half-float A, B, G and R and a
//! float Z.
//!
//! ```sh
//! cargo run --release --example bench
//! ```
//!
//! Each figure is the best of several runs, in megapixels per second.
use openexr_nrp::PixelType;
use openexr_nrp::dwa::{AcCompression, Channel, compress, decompress};
use std::time::Instant;

const RUNS: usize = 5;

/// A block of `lines` lines of `width` pixels: half-float A, B, G, R (smooth, with a little
/// noise) and a float Z, each line its channels one after the other.
fn block(width: usize, lines: usize, top: usize) -> Vec<u8> {
    let mut state = 1u64 + top as u64;
    let mut out = Vec::new();
    for y in top..top + lines {
        for c in 0..4 {
            for x in 0..width {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let noise = (state >> 60) as u16;
                let value: u16 = if c == 0 {
                    0x3C00
                } else {
                    0x3000 + ((x * (c + 1) + y * (4 - c)) / 3) as u16 % 0x0C00 + noise
                };
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for x in 0..width {
            out.extend_from_slice(&(10.0f32 + (x + y) as f32 * 0.01).to_le_bytes());
        }
    }
    out
}

fn main() {
    let width = 1024;
    let names = ["A", "B", "G", "R", "Z"];
    let channels: Vec<Channel> = names
        .iter()
        .map(|&name| Channel {
            name,
            pixel_type: if name == "Z" {
                PixelType::Float
            } else {
                PixelType::Half
            },
            linear: false,
        })
        .collect();
    for (name, lines) in [("dwaa", 32usize), ("dwab", 256)] {
        let blocks: Vec<Vec<u8>> = (0..512 / lines)
            .map(|b| block(width, lines, b * lines))
            .collect();
        let bytes: usize = blocks.iter().map(Vec::len).sum();
        let (mut ct, mut dt) = (f64::MAX, f64::MAX);
        let mut packed_len = 0;
        for _ in 0..RUNS {
            let start = Instant::now();
            let packed: Vec<Vec<u8>> = blocks
                .iter()
                .map(|b| {
                    compress(b, &channels, width, lines, 45.0, AcCompression::Huffman).unwrap()
                })
                .collect();
            ct = ct.min(start.elapsed().as_secs_f64());
            packed_len = packed.iter().map(Vec::len).sum();
            let start = Instant::now();
            for p in &packed {
                let mut out = vec![0u8; blocks[0].len()];
                decompress(p, &channels, width, lines, &mut out).unwrap();
            }
            dt = dt.min(start.elapsed().as_secs_f64());
        }
        let mpx = (width * 512) as f64 / 1e6;
        println!(
            "{name}  compress {:6.2} Mpixel/s   decompress {:6.2} Mpixel/s   {:.1}% of {bytes}",
            mpx / ct,
            mpx / dt,
            100.0 * packed_len as f64 / bytes as f64
        );
    }
}
