#!/usr/bin/env bash
# Pack a Windows build of the patched kicad-cli (MSYS2 UCRT64) into a self-contained
# engine: the program, its kifaces and every DLL it loads from MSYS2, next to each other.
#
#     plugin/engine/bundle-windows.sh BUILD_DIR SOURCE_DIR OUT.tar.xz
#
# Runs in an MSYS2 UCRT64 shell. Windows finds DLLs in the program's own folder first,
# so the engine needs nothing installed.
set -euo pipefail
build=$1 src=$2 out=$(realpath -m "$3")
E=$(mktemp -d)/pcbferry-engine
mkdir -p "$E/bin" "$E/share/kicad/template" "$E/share/kicad/schemas" "$E/share/doc/pcbferry"

cp "$build/kicad/kicad-cli.exe" "$E/bin/"
cp "$build"/eeschema/_eeschema.kiface "$build"/pcbnew/_pcbnew.kiface "$build"/cvpcb/_cvpcb.kiface "$E/bin/"
find "$build" -name '*.dll' \( -path '*/common/*' -o -path '*/api/*' -o -path '*/3d-viewer/*' \) \
    -exec cp {} "$E/bin/" \;

# Every MSYS2 DLL the program and kifaces load, transitively (Windows' own stay out)
for _ in 1 2 3 4; do
    for f in "$E"/bin/*.exe "$E"/bin/*.kiface "$E"/bin/*.dll; do
        ldd "$f" 2>/dev/null | awk '/=> \/ucrt64\//{print $3}'
    done | sort -u | while read -r dll; do
        [ -e "$E/bin/$(basename "$dll")" ] || cp "$dll" "$E/bin/"
    done
done

cp "$src/resources/project_template/kicad.kicad_pro" "$E/share/kicad/template/"
cp "$src"/api/schemas/*.json "$src"/resources/schemas/*.json "$E/share/kicad/schemas/"
cp "$src"/LICENSE* "$E/share/doc/pcbferry/"

"$E/bin/kicad-cli.exe" version
tar -C "$(dirname "$E")" -cf - pcbferry-engine | xz -9 -T0 > "$out"
ls -la "$out"
