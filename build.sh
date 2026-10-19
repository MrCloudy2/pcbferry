#!/usr/bin/env bash
# Rebuild the pcbferry image with the latest importer patches.
#
#   ./build.sh                 build from the patches/ in this repo, smoke-test
#   ./build.sh --patches DIR   first sync patches/ from DIR (patch development)
#   ./build.sh --no-sync       never sync, even when PCBFERRY_PATCHES is set
#
# Syncing makes patches/ hold exactly the NNNN-*.patch files of the source
# folder (new ones added, changed ones replaced, removed ones deleted); the
# image applies all of them in sorted order. Commit patches/ afterwards.
#
# Environment: PCBFERRY_PATCHES (patch folder to sync from; default: the
#              development folder ../altium-kicad-compat/kicad-patches if it exists),
#              PCBFERRY_IMAGE (default pcbferry), PCBFERRY_ENGINE
#              (podman or docker), JOBS (parallel compile jobs, default 4).
set -euo pipefail
export LC_ALL=C

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
src=${PCBFERRY_PATCHES:-$here/../altium-kicad-compat/kicad-patches}
image=${PCBFERRY_IMAGE:-pcbferry}
engine=${PCBFERRY_ENGINE:-$(command -v podman >/dev/null && echo podman || echo docker)}
# Sync only when there is a patch folder to sync from; otherwise build what is committed.
sync=
[[ -n ${PCBFERRY_PATCHES:-} || -d $src ]] && sync=1

while [[ $# -gt 0 ]]; do
    case $1 in
        --no-sync) sync= ;;
        --patches) src=$2; sync=1; shift ;;
        -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "build.sh: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

dest=$here/patches
mkdir -p "$dest"
if [[ -n $sync ]]; then
    [[ -d $src ]] || { echo "build.sh: patch folder $src not found (use --patches or --no-sync)" >&2; exit 2; }
    shopt -s nullglob
    new=("$src"/[0-9][0-9][0-9][0-9]-*.patch)
    [[ ${#new[@]} -gt 0 ]] || { echo "build.sh: no NNNN-*.patch in $src" >&2; exit 2; }
    for old in "$dest"/[0-9][0-9][0-9][0-9]-*.patch; do
        [[ -e $src/${old##*/} ]] || { echo "removed  ${old##*/}"; rm -- "$old"; }
    done
    for p in "${new[@]}"; do
        name=${p##*/}
        if [[ ! -e $dest/$name ]]; then echo "added    $name"; cp -- "$p" "$dest/"
        elif ! cmp -s "$p" "$dest/$name"; then echo "updated  $name"; cp -- "$p" "$dest/"
        fi
    done
    shopt -u nullglob
fi

patches=("$dest"/[0-9][0-9][0-9][0-9]-*.patch)
[[ -e ${patches[0]} ]] || { echo "build.sh: no patches in $dest" >&2; exit 2; }
set_id=$(cd "$dest" && cat [0-9][0-9][0-9][0-9]-*.patch | sha256sum | cut -c1-12)
echo "building $image with ${#patches[@]} patches (${patches[0]##*/} .. ${patches[-1]##*/}), set $set_id"

"$engine" build -f "$here/Containerfile" \
    --build-arg "JOBS=${JOBS:-4}" \
    --label "pcbferry.patches=$set_id" \
    -t "$image:latest" -t "$image:patches-$set_id" "$here"

echo "smoke test"
"$engine" run --rm --entrypoint kicad-cli "$image:latest" version
"$engine" run --rm --entrypoint kicad-cli "$image:latest" sch import --help >/dev/null
"$engine" run --rm "$image:latest" --version

echo "built $image:latest = $image:patches-$set_id"
if git -C "$here" status --porcelain -- patches | grep -q .; then
    echo "patches/ changed; review and commit:"
    git -C "$here" status --short -- patches
fi
