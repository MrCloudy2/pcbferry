#!/bin/sh
# Pack the image's patched kicad-cli into a self-contained Linux engine:
#
#     plugin/engine/bundle-linux.sh IMAGE OUT.tar.xz
#
# Every shared library it loads goes in, glibc and its loader too, so it runs on any
# x86-64 distribution whatever its glibc. bin/kicad-cli starts the real binary through
# the bundled loader; the loader sits in bin/ so KiCad, which finds its kifaces and data
# from /proc/self/exe, sees bin/ as its own folder.
set -eu
IMAGE=$1
OUT=$(realpath -m "$2")
ENGINE=${PCBFERRY_ENGINE:-$(command -v podman || command -v docker)}
here=$(dirname "$(realpath "$0")")

"$ENGINE" run --rm --entrypoint /bin/sh -v "$here:/scripts:ro" -v "$(dirname "$OUT"):/out" \
    "$IMAGE" /scripts/bundle-linux-inside.sh "/out/$(basename "$OUT")"
