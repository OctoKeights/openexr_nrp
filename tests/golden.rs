//! openexr_nrp against hashes recorded from OpenEXRCore, so that any change in output shows
//! without a C compiler.
//!
//! - `tests/data/dwa_write_expected.txt` is what `compare/dump_dwa.c`, built against
//!   OpenEXRCore of OpenEXR v3.5.4, prints for `dwa_write 3 400` and `dwa_write 8 400`:
//!   hashes of synthetic blocks compressed with DWAA and DWAB, and of what they decompress to.
//!
//! - `tests/data/dwa_read_expected.txt` is what it prints for `dwa_read 3 600` and
//!   `dwa_read 8 600`: hashes of what the same blocks, damaged, decompress to, or the error.
//!
//! `compare/compare.sh` runs the full comparison.

mod made_up;

fn same(printed: String, expected: &str) {
    for (line, (ours, theirs)) in printed.lines().zip(expected.lines()).enumerate() {
        assert_eq!(ours, theirs, "line {}", line + 1);
    }
    assert_eq!(printed.lines().count(), expected.lines().count());
}

#[test]
fn compresses_blocks_as_openexr_does() {
    same(
        [
            made_up::dwa("dwa_write", 3, 400),
            made_up::dwa("dwa_write", 8, 400),
        ]
        .concat(),
        include_str!("data/dwa_write_expected.txt"),
    );
}

#[test]
fn decompresses_damaged_blocks_as_openexr_does() {
    same(
        [
            made_up::dwa("dwa_read", 3, 600),
            made_up::dwa("dwa_read", 8, 600),
        ]
        .concat(),
        include_str!("data/dwa_read_expected.txt"),
    );
}
