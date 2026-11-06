#!/usr/bin/env bash
# Pack a macOS build of the patched kicad-cli into a self-contained engine:
#
#     plugin/engine/bundle-macos.sh BUILD_DIR SOURCE_DIR OUT.tar.xz
#
# KiCad on macOS finds its kifaces from the app bundle it runs in, so the engine is a
# minimal KiCad.app: Contents/MacOS/kicad-cli, Contents/PlugIns/*.kiface, and every
# Homebrew library they load in Contents/Frameworks (install names rewritten by
# dylibbundler). bin/kicad-cli points KiCad at the bundled data and starts it.
# Everything is signed ad hoc, which Apple Silicon requires of any code it runs.
set -euo pipefail
build=$1 src=$2 out=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$3")
E=$(mktemp -d)/pcbferry-engine
app=$E/KiCad.app/Contents
mkdir -p "$app/MacOS" "$app/PlugIns" "$app/Frameworks" "$E/bin" \
         "$E/share/kicad/template" "$E/share/kicad/schemas" "$E/share/doc/pcbferry"

# On macOS the build already lays things out in app bundles: find them wherever they are
one() { find "$build" -name "$1" -type f -not -path '*/CMakeFiles/*' | head -1; }
cli=$(one kicad-cli)
[ -n "$cli" ] || { echo "kicad-cli not found in $build" >&2; exit 1; }
cp "$cli" "$app/MacOS/"
for k in _eeschema _pcbnew _cvpcb; do
    f=$(one "$k.kiface")
    [ -n "$f" ] || { echo "$k.kiface not found in $build" >&2; exit 1; }
    cp "$f" "$app/PlugIns/"
done
find "$build" -type f \( -name 'libkicommon*.dylib' -o -name 'libkigal*.dylib' -o -name 'libkiapi*.dylib' \
    -o -name 'libkicad_3dsg*.dylib' \) -not -path '*/CMakeFiles/*' -exec cp {} "$app/Frameworks/" \;
ls -la "$app/MacOS" "$app/PlugIns" "$app/Frameworks"

# Copy the libraries KiCad's own outputs load and rewrite every reference to them
args=()
for f in "$app"/MacOS/kicad-cli "$app"/PlugIns/*.kiface "$app"/Frameworks/*.dylib; do
    args+=(-x "$f")
done
dylibbundler -of -b "${args[@]}" -d "$app/Frameworks" -p @executable_path/../Frameworks/ \
    -s "$build/common" -s "$build/api" -s "$build/common/gal" -s "$build/3d-viewer/3d_cache/sg"

find "$app" -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.kiface' \) -exec codesign --force -s - {} \;

cp "$src/resources/project_template/kicad.kicad_pro" "$E/share/kicad/template/"
cp "$src"/api/schemas/*.json "$src"/resources/schemas/*.json "$E/share/kicad/schemas/"
cp "$src"/LICENSE* "$E/share/doc/pcbferry/"

cat > "$E/bin/kicad-cli" <<'SH'
#!/bin/sh
d=$(cd "$(dirname "$0")" && pwd -P)
KICAD_STOCK_DATA_HOME=$d/../share/kicad export KICAD_STOCK_DATA_HOME
exec "$d/../KiCad.app/Contents/MacOS/kicad-cli" "$@"
SH
chmod 755 "$E/bin/kicad-cli"

"$E/bin/kicad-cli" version
tar -C "$(dirname "$E")" -cf - pcbferry-engine | xz -9 -T0 > "$out"
ls -la "$out"
