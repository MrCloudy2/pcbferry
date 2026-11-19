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

# dylibbundler leaves frameworks alone; KiCad links Python's (never starts it in kicad-cli),
# so the library itself goes in Frameworks too
machos() { find "$app" -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.kiface' \); }
for f in $(machos); do
    otool -L "$f" | awk 'NR>1 && /Python\.framework/{print $1}' | while read -r ref; do
        base=libpython$(basename "$(dirname "$ref")").dylib
        if [ ! -e "$app/Frameworks/$base" ]; then
            cp "$ref" "$app/Frameworks/$base"
            chmod u+w "$app/Frameworks/$base"
            install_name_tool -id "@executable_path/../Frameworks/$base" "$app/Frameworks/$base"
        fi
        install_name_tool -change "$ref" "@executable_path/../Frameworks/$base" "$f"
    done
done

# Each bundled library names itself by where it now is (otool -L lists that name too)
for f in "$app"/Frameworks/*.dylib; do
    install_name_tool -id "@executable_path/../Frameworks/$(basename "$f")" "$f"
done

# Nothing may still point outside the bundle and the system
bad=0
for f in $(machos); do
    if otool -L "$f" | awk 'NR>1{print $1}' | grep -Ev '^(/usr/lib/|/System/|@executable_path/|@loader_path/|@rpath/)'; then
        echo "  ^ referenced by $f" >&2; bad=1
    fi
done
[ $bad = 0 ] || { echo "the bundle still loads libraries from outside itself" >&2; exit 1; }

machos | while read -r f; do codesign --force -s - "$f"; done

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
