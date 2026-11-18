//! The Rust half of the comparison with OpenEXRCore: runs the port over synthetic data and
//! prints a running hash of what comes out, as `compare/dump_dwa.c` does with the original.
//! `compare/compare.sh` compares the two.
//!
//!   dump `dwa_write` <seed> <count>   synthetic blocks of one to six channels (of halves, floats
//!                                   and integers, named as DWA's rules have them and not)
//!                                   compressed as DWAA or DWAB at a synthetic level, and back
//!   dump `dwa_read` <seed> <count>    those blocks, compressed, damaged in one of eleven ways,
//!                                   decompressed

#[path = "../tests/made_up/mod.rs"]
mod made_up;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(what), Some(seed), Some(count)) = (
        args.get(1),
        args.get(2).and_then(|s| s.parse::<u64>().ok()),
        args.get(3).and_then(|s| s.parse::<u32>().ok()),
    ) else {
        eprintln!("usage: dump <what> <seed> <count>   (see the top of dump.rs)");
        std::process::exit(2);
    };
    let _ = seed;
    match what.as_str() {
        "dwa_write" | "dwa_read" => print!("{}", made_up::dwa(what, seed, count)),
        _ => {
            eprintln!("unknown dump {what}");
            std::process::exit(2);
        }
    }
}
