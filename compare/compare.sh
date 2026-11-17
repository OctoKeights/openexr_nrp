#!/bin/sh
# Compares openexr_nrp with OpenEXRCore, of OpenEXR v3.5.4
# (https://github.com/AcademySoftwareFoundation/openexr).
#
#   OPENEXR_BUILD=<its CMake build folder> compare/compare.sh <checkout of OpenEXR> [count] [seeds] [dumps]
#
# compare/dump_dwa.c is built against the OpenEXRCore of a CMake build of OpenEXR (with Imath as
# that build fetches it; only OpenEXRCore need be built), and examples/dump.rs from the port; both
# run the same synthetic data for each seed and what they print is compared byte for byte. Dumps:
# dwa_write, with a hundredth of [count] (200,000 by default) as the number of synthetic blocks,
# each written by OpenEXRCore as the one block of a file, of lines or of a tile, compressed with
# DWAA or DWAB and decompressed; and dwa_read, with as many, each then damaged and
# decompressed. OpenEXRCore picks its code by what the machine has: the port is of its code for
# x86-64 with AVX and F16C, and elsewhere its blocks differ.
set -u
if [ $# -lt 1 ]; then
    echo "usage: $0 <checkout of OpenEXR> [count] [seeds] [dumps]" >&2
    exit 2
fi
here=$(cd "$(dirname "$0")/.." && pwd)
source=$1
blocks=${2:-200000}
seeds=${3:-3}
dumps=${4:-dwa_write dwa_read}
tmp=${TMPDIR:-/tmp}/openexr_nrp-compare
mkdir -p "$tmp" || exit 2
if ! [ -f "$source/src/lib/OpenEXRCore/openexr.h" ]; then
    echo "$source does not look like checkout of OpenEXR" >&2
    exit 2
fi

(cd "$here" && cargo build --release -q --ignore-rust-version --example dump) || exit 2
rust=$(cd "$here" && cargo metadata --format-version 1 --no-deps 2>/dev/null | sed 's/.*"target_directory":"\([^"]*\)".*/\1/')/release/examples/dump

core=$(find "${OPENEXR_BUILD:-/nonexistent}" -name 'libOpenEXRCore*.a' 2> /dev/null | head -1)
imath=$(find "${OPENEXR_BUILD:-/nonexistent}" -path '*/Imath/ImathConfig.h' 2> /dev/null | head -1)
if [ -z "$core" ] || [ -z "$imath" ] || [ ! -f "${OPENEXR_BUILD:-}/cmake/OpenEXRConfig.h" ]; then
    echo "no build of OpenEXR: put its CMake build folder, with OpenEXRCore built, in \$OPENEXR_BUILD" >&2
    exit 2
fi
${CC:-cc} -O2 -I "$source/src/lib/OpenEXRCore" -I "$OPENEXR_BUILD/cmake" -I "$(dirname "$(dirname "$imath")")" -c "$here/compare/dump_dwa.c" -o "$tmp/dump_dwa.o" || exit 2
${CXX:-c++} "$tmp/dump_dwa.o" "$core" -lm -o "$tmp/dump-dwa" || exit 2

bad=0
for what in $dumps; do
    extra=
    case $what in
    dwa_write | dwa_read)
        reference=$tmp/dump-dwa
        count=$((blocks / 100))
        ;;
    *)
        echo "unknown dump $what" >&2
        exit 2
        ;;
    esac
    seed=1
    while [ "$seed" -le "$seeds" ]; do
        # shellcheck disable=SC2086
        "$reference" "$what" "$seed" "$count" $extra > "$tmp/reference.txt" 2> /dev/null
        # shellcheck disable=SC2086
        "$rust" "$what" "$seed" "$count" $extra > "$tmp/rust.txt"
        if [ ! -s "$tmp/reference.txt" ]; then
            echo "$what, seed $seed: NOT COMPARED, the original's side printed nothing"
            bad=$((bad + 1))
        elif cmp -s "$tmp/reference.txt" "$tmp/rust.txt"; then
            echo "$what, seed $seed: the same ($(tail -1 "$tmp/rust.txt"))"
        else
            echo "$what, seed $seed: DIFFERENT"
            bad=$((bad + 1))
        fi
        seed=$((seed + 1))
    done
done
[ "$bad" -eq 0 ]
