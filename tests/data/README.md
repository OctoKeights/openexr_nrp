# Recorded results

`dwa_write_expected.txt` holds the hashes `compare/dump_dwa.c` prints when built against
OpenEXRCore of OpenEXR v3.5.4, for synthetic blocks compressed with DWAA and DWAB and
decompressed; `dwa_read_expected.txt` the hashes for the same blocks damaged and
decompressed. `tests/golden.rs` checks that openexr_nrp prints the same.

To record them again, on x86-64 with AVX and F16C, run the comparison once, which builds
OpenEXRCore's side, then run that program directly:

    OPENEXR_BUILD=<its CMake build folder> compare/compare.sh <checkout of OpenEXR> 3000 1
    ref="${TMPDIR:-/tmp}/openexr_nrp-compare/dump-dwa"
    { "$ref" dwa_write 3 400; "$ref" dwa_write 8 400; } > tests/data/dwa_write_expected.txt
    { "$ref" dwa_read 3 600; "$ref" dwa_read 8 600; } > tests/data/dwa_read_expected.txt

All test data is generated from seeds; no external files are used.
