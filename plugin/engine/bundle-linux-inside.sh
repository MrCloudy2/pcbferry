#!/bin/sh
# Runs inside the pcbferry image; see bundle-linux.sh.
set -eu
P=/opt/kicad-altium
E=/tmp/pcbferry-engine
rm -rf $E; mkdir -p $E/bin $E/lib $E/share $E/fonts

cp -a $P/share/kicad $E/share/
cp -a $P/share/doc $E/share/
# The source tarball (180 MB) is attached to every release next to the engine instead
rm -f $E/share/doc/pcbferry/*.tar.xz
cp $P/bin/_*.kiface $E/bin/
cp $P/bin/kicad-cli $E/bin/kicad-cli.bin

# The library closure, resolved by the image's own loader
# (libSDL3: Arch's SDL2 is sdl2-compat, which dlopens it)
ldd $P/bin/kicad-cli $P/bin/_*.kiface /usr/lib/libSDL3.so.0 | awk '/=> \//{print $3} /^\t\/.*ld-linux/{print $1}' \
    | sort -u | while read -r lib; do cp -L "$lib" $E/lib/; done
cp -L /usr/lib/libSDL3.so.0 $E/lib/
mv $E/lib/ld-linux-x86-64.so.2 $E/bin/

# Fonts: Liberation (Arial's metrics, which Altium boards use) plus the host's own
cp /usr/share/fonts/liberation/*.ttf $E/fonts/
cat > $E/fonts/fonts.conf <<'XML'
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <dir prefix="relative">.</dir>
  <alias binding="same"><family>Arial</family><prefer><family>Liberation Sans</family></prefer></alias>
  <alias binding="same"><family>Times New Roman</family><prefer><family>Liberation Serif</family></prefer></alias>
  <alias binding="same"><family>Courier New</family><prefer><family>Liberation Mono</family></prefer></alias>
  <cachedir prefix="xdg">pcbferry-fontconfig</cachedir>
</fontconfig>
XML

cat > $E/bin/kicad-cli <<'SH'
#!/bin/sh
d=$(dirname "$(readlink -f "$0")")
FONTCONFIG_FILE=$d/../fonts/fonts.conf export FONTCONFIG_FILE
KICAD_STOCK_DATA_HOME=$d/../share/kicad export KICAD_STOCK_DATA_HOME
exec "$d/ld-linux-x86-64.so.2" --library-path "$d/../lib" --argv0 "$0" "$d/kicad-cli.bin" "$@"
SH
chmod 755 $E/bin/kicad-cli $E/bin/ld-linux-x86-64.so.2

$E/bin/kicad-cli version
tar -C /tmp -cf - pcbferry-engine | xz -9 -T0 > "$1"
ls -la "$1"
