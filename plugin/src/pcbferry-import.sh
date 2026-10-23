#!/bin/sh
# KiCad starts this (exec runtime). Run the plugin with the system's Python, outside
# whatever environment KiCad itself runs in: AppImages point PYTHONHOME at their own
# Python and preload their own libraries, which break other programs.
unset PYTHONHOME PYTHONPATH LD_PRELOAD LD_LIBRARY_PATH GIO_MODULE_DIR GIO_EXTRA_MODULES \
    GDK_PIXBUF_MODULE_FILE
cd "$(dirname "$0")" || exit 1
for py in python3 python; do
    command -v "$py" >/dev/null 2>&1 && exec "$py" import_altium.py
done
msg="pcbferry needs Python 3, which isn't installed."
command -v kdialog >/dev/null 2>&1 && exec kdialog --title pcbferry --error "$msg"
command -v zenity >/dev/null 2>&1 && exec zenity --error --title pcbferry --text "$msg"
command -v osascript >/dev/null 2>&1 && exec osascript -e "display dialog \"$msg\" buttons {\"OK\"}"
echo "$msg" >&2
exit 1
