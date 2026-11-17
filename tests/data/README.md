# Recorded results

`dwa_write_expected.txt` is what OpenEXRCore makes of made-up blocks of an image with DWAA and
DWAB and back, as `compare/dump_dwa.c` prints it, and `dwa_read_expected.txt` what it makes of
them damaged; `tests/golden.rs` checks that the port prints the same. To make them again (on
x86-64 with AVX and F16C), run the comparison once (it builds the original's side), then that
by itself:

    OPENEXR_BUILD=<its CMake build folder> compare/compare.sh <checkout of OpenEXR> 3000 1
    ref="${TMPDIR:-/tmp}/openexr_nrp-compare/dump-dwa"
    { "$ref" dwa_write 3 400; "$ref" dwa_write 8 400; } > tests/data/dwa_write_expected.txt
    { "$ref" dwa_read 3 600; "$ref" dwa_read 8 600; } > tests/data/dwa_read_expected.txt

No picture or file from anywhere is in this repository: every file and stream the tests and
the comparison use is made up from a seed.
