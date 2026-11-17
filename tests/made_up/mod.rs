//! Made-up data, the same from a seed here as in the C side of the comparison
//! (`compare/`), and the dumps the tests run as well as the comparison (`examples/dump.rs`).

// (The arithmetic is written as the C side has it; not every user of this module uses all of
// it.)
#![allow(
    clippy::manual_is_multiple_of,
    clippy::needless_range_loop,
    clippy::manual_div_ceil,
    clippy::manual_checked_ops,
    dead_code
)]

/// The two programs make the same blocks from the same seed: a 64-bit linear congruential
/// generator, the high half of its state.
pub struct Random {
    pub state: u64,
}

impl Random {
    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
}

/// FNV-1a, 64 bits.
pub struct Hash {
    pub value: u64,
}

impl Hash {
    pub fn new() -> Hash {
        Hash {
            value: 14695981039346656037,
        }
    }

    pub fn add(&mut self, data: &[u8]) {
        for &byte in data {
            self.value ^= u64::from(byte);
            self.value = self.value.wrapping_mul(1099511628211);
        }
    }
}

/// The `dwa_write` and `dwa_read` dumps of `compare/dump_dwa.c`: made-up blocks of an image
/// compressed as DWAA or DWAB and back (`dwa_write`), or compressed, damaged and decompressed
/// (`dwa_read`).
pub fn dwa(what: &str, seed: u64, count: u32) -> String {
    use openexr_nrp::dwa::{Channel, compress, uncompress};
    const NAMES: [&str; 24] = [
        "R",
        "G",
        "B",
        "A",
        "Y",
        "BY",
        "RY",
        "Z",
        "r",
        "g",
        "b",
        "a",
        "red",
        "green",
        "blue",
        "diffuse.R",
        "diffuse.G",
        "diffuse.B",
        "diffuse.A",
        "spec.R",
        "spec.B",
        "left.Y",
        "mask",
        "N.x",
    ];
    let read = what == "dwa_read";
    let mut r = Random { state: seed };
    let mut hash = Hash::new();
    let mut out = String::new();
    let (mut raw_total, mut block_total, mut stored) = (0u64, 0u64, 0u64);
    let (mut decompressed, mut refused) = (0u64, 0u64);

    // The generator's own half of a float: to nearest, ties to even.
    fn to_half(f: f32) -> u16 {
        let bits = f.to_bits();
        let sign = (bits >> 16) & 0x8000;
        let ui = bits & 0x7fff_ffff;
        if ui > 0x7f80_0000 {
            return (sign | 0x7e00 | ((ui >> 13) & 0x3ff)) as u16;
        }
        if ui == 0x7f80_0000 {
            return (sign | 0x7c00) as u16;
        }
        let exp = (ui >> 23) as i32;
        let mant = ui & 0x7f_ffff;
        if exp >= 113 {
            let mut h = (((exp - 112) as u32) << 10) | (mant >> 13);
            let rest = mant & 0x1fff;
            if rest > 0x1000 || (rest == 0x1000 && h & 1 != 0) {
                h += 1;
            }
            return (sign | h.min(0x7c00)) as u16;
        }
        if exp < 102 {
            return sign as u16;
        }
        let m = mant | 0x80_0000;
        let shift = (126 - exp) as u32;
        let mut h = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        if rest > halfway || (rest == halfway && h & 1 != 0) {
            h += 1;
        }
        (sign | h) as u16
    }

    for i in 0..count {
        // MakeBlock
        let compression = if r.next() % 2 != 0 { 8 } else { 9 };
        let tiled = r.next() % 4 == 0;
        let width = 1 + match r.next() % 8 {
            0 => r.next() % 8,
            1..=4 => r.next() % 64,
            5 | 6 => r.next() % 160,
            _ => r.next() % 400,
        } as usize;
        let most = if compression == 8 { 32u32 } else { 256 };
        let few = r.next() % 8 < 6;
        let lines = 1 + (r.next() % if few { most.min(40) } else { most }) as usize;
        let level = match r.next() % 8 {
            4 => 0.0f32,
            5 => (r.next() % 300) as f32 + (r.next() % 100) as f32 / 100.0,
            6 => (r.next() % 100000) as f32 / 7.0,
            7 => (r.next() % 2000) as f32 / 10.0,
            _ => 45.0,
        };
        let n = 1 + (r.next() % 6) as usize;
        let mut names: Vec<usize> = Vec::new();
        while names.len() < n {
            let name = (r.next() % 24) as usize;
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.sort_by(|a, b| NAMES[*a].as_bytes().cmp(NAMES[*b].as_bytes()));
        let mut types = Vec::new();
        let mut linear = Vec::new();
        let mut planes: Vec<Vec<u8>> = Vec::new();
        for _ in 0..n {
            let t = r.next() % 10;
            let pixel_type = if t < 6 {
                1
            } else if t < 9 {
                2
            } else {
                0
            };
            let lin = r.next() % 8 == 0;
            let bpe = if pixel_type == 1 { 2 } else { 4 };
            let kind = r.next() % 8;
            let (a, b, c) = (r.next() % 32, r.next() % 32, r.next() % 1000);
            let mut plane = Vec::with_capacity(width * lines * bpe);
            for y in 0..lines {
                for x in 0..width {
                    if pixel_type == 0 {
                        let u = if kind < 4 { r.next() % 256 } else { r.next() };
                        plane.extend_from_slice(&u.to_le_bytes());
                        continue;
                    }
                    if kind == 4 || kind == 5 {
                        let mut bits = r.next();
                        if pixel_type == 1 || kind == 4 {
                            let mut h = bits as u16;
                            if lin && (h & 0x7c00) == 0x7c00 {
                                h &= 0xfbff;
                            }
                            if pixel_type == 1 {
                                plane.extend_from_slice(&h.to_le_bytes());
                                continue;
                            }
                            let h = u32::from(h);
                            let (sign, e, m) = ((h >> 15) << 31, (h >> 10) & 0x1f, h & 0x3ff);
                            bits = if e == 0x1f {
                                sign | 0x7f80_0000 | (m << 13)
                            } else if e != 0 {
                                sign | ((e + 112) << 23) | (m << 13)
                            } else {
                                (m as f32 / 16777216.0).to_bits() | sign
                            };
                        } else if lin && (bits & 0x7f80_0000) == 0x7f80_0000 {
                            bits = 0;
                        }
                        plane.extend_from_slice(&bits.to_le_bytes());
                        continue;
                    }
                    let f = match kind {
                        0 => {
                            ((x as i32 * a as i32 + y as i32 * b as i32 + c as i32) as u32 % 4096)
                                as f32
                                / 1000.0
                                - 1.0
                        }
                        1 => (r.next() % 20000) as f32 / 5000.0 - 2.0,
                        2 => a as f32 / 100.0,
                        3 => {
                            if r.next() % 16 == 0 {
                                (r.next() % 1000) as f32 / 50.0
                            } else {
                                0.0
                            }
                        }
                        6 => (((x / 8 + y / 8) as u32 + a) % 3) as f32 * 0.5,
                        _ => {
                            let f = (r.next() % 60000) as f32 + 0.5;
                            if r.next() % 2 != 0 { f } else { -f }
                        }
                    };
                    if pixel_type == 1 {
                        plane.extend_from_slice(&to_half(f).to_le_bytes());
                    } else {
                        plane.extend_from_slice(&f.to_bits().to_le_bytes());
                    }
                }
            }
            types.push(pixel_type);
            linear.push(lin);
            planes.push(plane);
        }
        let channels: Vec<Channel> = (0..n)
            .map(|c| Channel {
                name: NAMES[names[c]],
                pixel_type: types[c],
                p_linear: linear[c],
            })
            .collect();

        // RawBlock
        let mut raw = Vec::new();
        for y in 0..lines {
            for c in 0..n {
                let row = width * if types[c] == 1 { 2 } else { 4 };
                raw.extend_from_slice(&planes[c][row * y..row * (y + 1)]);
            }
        }

        let mut block = compress(
            &raw,
            &channels,
            width,
            lines,
            level,
            compression == 8 && tiled,
        );
        raw_total += raw.len() as u64;
        block_total += block.len() as u64;
        stored += u64::from(block.len() == raw.len());
        let mut decode = |hash: &mut Hash, data: &[u8]| {
            let mut lines_out = vec![0u8; raw.len()];
            let code = match uncompress(data, &channels, width, lines, &mut lines_out) {
                Ok(()) => {
                    decompressed += 1;
                    0
                }
                Err(code) => {
                    refused += 1;
                    code
                }
            };
            hash.add(&(code as u32).to_le_bytes());
            hash.add(&lines_out);
        };
        if !read {
            hash.add(&(raw.len() as u32).to_le_bytes());
            hash.add(&raw);
            hash.add(&(block.len() as u32).to_le_bytes());
            hash.add(&block);
            decode(&mut hash, &block);
        } else {
            // Damage
            let size = block.len();
            let room = size + 400;
            block.resize(room, 0);
            let put = |data: &mut [u8], i: usize, value: u64| {
                data[8 * i..8 * i + 8].copy_from_slice(&value.to_le_bytes())
            };
            let get = |data: &[u8], i: usize| {
                u64::from_le_bytes(data[8 * i..8 * i + 8].try_into().unwrap())
            };
            let size = match r.next() % 11 {
                10 => 'rules: {
                    if size < 90 || get(&block, 0) != 2 {
                        break 'rules size;
                    }
                    let old = usize::from(block[88]) | (usize::from(block[89]) << 8);
                    if old < 2 || 88 + old > size {
                        break 'rules size;
                    }
                    let mut rules = vec![0u8, 0];
                    let many = 1 + r.next() % 6;
                    for _ in 0..many {
                        let suffix = NAMES[(r.next() % 15) as usize];
                        rules.extend_from_slice(suffix.as_bytes());
                        rules.push(0);
                        let (csc, scheme, nocase) = (r.next() % 4, r.next() % 3, r.next() % 2);
                        rules.push(((csc << 4) | (scheme << 2) | nocase) as u8);
                        rules.push((r.next() % 3) as u8);
                    }
                    let n = rules.len();
                    rules[0] = n as u8;
                    let rest = size - 88 - old;
                    if 88 + n + rest > room {
                        break 'rules size;
                    }
                    block.copy_within(88 + old..size, 88 + n);
                    block[88..88 + n].copy_from_slice(&rules);
                    88 + n + rest
                }
                1 => {
                    let n = 1 + r.next() % 4;
                    for _ in 0..n {
                        if size == 0 {
                            break;
                        }
                        let at = r.next() as usize % size;
                        block[at] ^= (1 + r.next() % 255) as u8;
                    }
                    size
                }
                2 => {
                    if size > 0 {
                        r.next() as usize % size
                    } else {
                        0
                    }
                }
                3 => {
                    if size >= 88 {
                        let i = (r.next() % 11) as usize;
                        let value = match r.next() % 3 {
                            0 => u64::from(r.next() % 2000),
                            1 => get(&block, i)
                                .wrapping_add(u64::from(r.next() % 5))
                                .wrapping_sub(2),
                            _ => {
                                let low = u64::from(r.next());
                                low | (u64::from(r.next()) << 32)
                            }
                        };
                        put(&mut block, i, value);
                    }
                    size
                }
                4 => {
                    if size > 88 {
                        let span = (size - 88).min(40);
                        let at = 88 + r.next() as usize % span;
                        block[at] = r.next() as u8;
                    }
                    size
                }
                5 => {
                    let n = r.next() % 20;
                    let mut size = size;
                    for _ in 0..n {
                        if size >= room {
                            break;
                        }
                        block[size] = r.next() as u8;
                        size += 1;
                    }
                    size
                }
                6 => {
                    let n = (88 + r.next() % 300) as usize;
                    let n = n.min(room);
                    for k in 0..n {
                        block[k] = r.next() as u8;
                    }
                    if n >= 88 {
                        for i in 0..11 {
                            let value = if i == 0 { r.next() % 3 } else { r.next() % 400 };
                            put(&mut block, i, u64::from(value));
                        }
                    }
                    n
                }
                7 => {
                    if size > 120 {
                        let n = 1 + r.next() % 3;
                        for _ in 0..n {
                            let at = 120 + r.next() as usize % (size - 120);
                            block[at] ^= (1 + r.next() % 255) as u8;
                        }
                    }
                    size
                }
                8 => {
                    if size >= 88 {
                        let version = r.next() % 2;
                        put(&mut block, 0, u64::from(version));
                    }
                    size
                }
                9 => {
                    if size >= 88 {
                        let ac = r.next() % 3;
                        put(&mut block, 10, u64::from(ac));
                    }
                    size
                }
                _ => size,
            };
            hash.add(&(size as u32).to_le_bytes());
            decode(&mut hash, &block[..size]);
        }
        if (i + 1) % 200 == 0 || i + 1 == count {
            out += &format!("{what} {} {:016x}\n", i + 1, hash.value);
        }
    }
    out + &format!(
        "{what} bytes: {raw_total} as {block_total}, {stored} blocks stored as they are; decompressed: {decompressed}, refused: {refused}\n"
    )
}
