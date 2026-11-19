#!/usr/bin/env bash
# Pack a Windows build of the patched kicad-cli (MSYS2 UCRT64) into a self-contained
# engine: the program, its kifaces and every DLL it loads from MSYS2, next to each other.
#
#     plugin/engine/bundle-windows.sh BUILD_DIR SOURCE_DIR OUT.tar.xz
#
# Runs in an MSYS2 UCRT64 shell. Windows finds DLLs in the program's own folder first,
# so the engine needs nothing installed.
set -euo pipefail -x
build=$1 src=$2 out=$(realpath -m "$3")
E=$(mktemp -d)/pcbferry-engine
mkdir -p "$E/bin" "$E/share/kicad/template" "$E/share/kicad/schemas" "$E/share/doc/pcbferry"

cp "$build/kicad/kicad-cli.exe" "$E/bin/"
# (kifaces are DLLs on Windows: _eeschema.dll, ...)
cp "$build"/eeschema/_eeschema.dll "$build"/pcbnew/_pcbnew.dll "$build"/cvpcb/_cvpcb.dll "$E/bin/"
find "$build" -name '*.dll' \( -path '*/common/*' -o -path '*/api/*' -o -path '*/3d-viewer/*' \) \
    -exec cp {} "$E/bin/" \;

# Every MSYS2 DLL the program and kifaces import, transitively (Windows' own stay out).
# Read from the import tables with objdump: ldd would load each DLL, which can hang.
todo=("$E"/bin/*.exe "$E"/bin/*.dll)
while [ ${#todo[@]} -gt 0 ]; do
    next=()
    for f in "${todo[@]}"; do
        for name in $(objdump -p "$f" | awk '/DLL Name:/{print $3}'); do
            [ -e "$E/bin/$name" ] && continue
            [ -e "/ucrt64/bin/$name" ] || continue      # a Windows system DLL
            cp "/ucrt64/bin/$name" "$E/bin/"
            next+=("$E/bin/$name")
        done
    done
    todo=("${next[@]+"${next[@]}"}")
done
ls "$E/bin" | wc -l

cp "$src/resources/project_template/kicad.kicad_pro" "$E/share/kicad/template/"
cp "$src"/api/schemas/*.json "$src"/resources/schemas/*.json "$E/share/kicad/schemas/"
cp "$src"/LICENSE* "$E/share/doc/pcbferry/"

timeout 300 "$E/bin/kicad-cli.exe" version
tar -C "$(dirname "$E")" -cf - pcbferry-engine | xz -9 -T0 > "$out"
ls -la "$out"
