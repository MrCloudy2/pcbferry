# altium2kicad: KiCad 10.0.7 + Altium importer patches, headless.
#
#   docker build -t altium2kicad -f Containerfile .      (or podman build)
#
# Builds only kicad-cli and the eeschema/pcbnew kifaces. Packages come from a
# dated Arch Linux Archive snapshot so a rebuild resolves the same versions;
# pass --build-arg ARCH_SNAPSHOT= to use the live mirrors instead.

ARG ARCH_IMAGE=docker.io/library/archlinux:base@sha256:3c64b538588416593f69a1aebd2f2ec0f2541b29159f32556fa7def2167147a0
ARG ARCH_SNAPSHOT=2026/10/12

# Runtime libraries of kicad-cli + kifaces (Arch's kicad package, minus the
# GUI-only wxPython). Shared by both stages.
ARG RUNTIME_PKGS="boost-libs fontconfig zstd abseil-cpp freetype2 libsecret glib2 gtk3 glu \
    cairo harfbuzz libglvnd libspnav wxwidgets-gtk3 zlib curl glm ngspice opencascade python \
    webkit2gtk-4.1 unixodbc libgit2 nng protobuf poppler-glib"

FROM ${ARCH_IMAGE} AS base
ARG ARCH_SNAPSHOT
RUN if [ -n "$ARCH_SNAPSHOT" ]; then \
        echo "Server = https://archive.archlinux.org/repos/${ARCH_SNAPSHOT}/\$repo/os/\$arch" \
            > /etc/pacman.d/mirrorlist; \
    fi

# ---------------------------------------------------------------- build ----
FROM base AS build
ARG RUNTIME_PKGS
RUN pacman -Syu --noconfirm --needed base-devel cmake ninja git swig boost \
        wayland-protocols ${RUNTIME_PKGS} \
    && pacman -Scc --noconfirm

ARG KICAD_TAG=10.0.7
ARG KICAD_COMMIT=69cafb907ebc09e7722a2a464b1d466fc2e3608d
WORKDIR /src
RUN git clone --depth 1 --branch "${KICAD_TAG}" https://gitlab.com/kicad/code/kicad.git kicad \
    && test "$(git -C kicad rev-parse HEAD)" = "${KICAD_COMMIT}"

COPY patches/ /src/patches/
# Every NNNN-*.patch, in byte order; anything else in patches/ is ignored.
RUN set -e; cd kicad; export LC_ALL=C; \
    set -- /src/patches/[0-9][0-9][0-9][0-9]-*.patch; \
    [ -e "$1" ] || { echo "no NNNN-*.patch in patches/" >&2; exit 1; }; \
    for p in "$@"; do echo "applying ${p##*/}"; git apply --whitespace=nowarn "$p"; done; \
    (cd /src/patches && sha256sum [0-9][0-9][0-9][0-9]-*.patch) > /src/patches.sha256

ARG PREFIX=/opt/kicad-altium
ARG JOBS=4
RUN cmake -S kicad -B build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=${PREFIX} \
        -DKICAD_BUILD_QA_TESTS=OFF \
        -DKICAD_SCRIPTING_WXPYTHON=OFF \
        -DKICAD_BUILD_I18N=OFF \
        -DKICAD_INSTALL_DEMOS=OFF \
        -DKICAD_UPDATE_CHECK=OFF \
        -DKICAD_USE_SENTRY=OFF \
        -DKICAD_VERSION_EXTRA=altium2kicad \
    && ninja -C build -j${JOBS} kicad-cli eeschema_kiface pcbnew_kiface api_schema_build_copy

# Install by hand: `ninja install` would build every GUI program first.
# Layout mirrors a normal install (kifaces next to the binary, data under share/kicad).
RUN set -e; D=/install${PREFIX}; \
    mkdir -p $D/bin $D/lib $D/share/kicad/template $D/share/kicad/schemas; \
    install -m755 build/kicad/kicad-cli build/eeschema/_eeschema.kiface \
        build/pcbnew/_pcbnew.kiface $D/bin/; \
    cp -P build/common/libkicommon.so* build/common/gal/libkigal.so* build/api/libkiapi.so* \
        build/3d-viewer/3d_cache/sg/libkicad_3dsg.so* $D/lib/; \
    cp kicad/resources/project_template/kicad.kicad_pro $D/share/kicad/template/; \
    cp kicad/api/schemas/*.json kicad/resources/schemas/*.json $D/share/kicad/schemas/

# Corresponding source (GPL-3.0+): the patched tree exactly as built, plus the patches.
RUN set -e; S=/install${PREFIX}/share/doc/altium2kicad; mkdir -p $S; \
    cp -r /src/patches $S/; \
    tar -C /src --exclude=.git -cJf $S/kicad-${KICAD_TAG}-altium2kicad-source.tar.xz kicad; \
    { echo "KiCad ${KICAD_TAG} (${KICAD_COMMIT}) from https://gitlab.com/kicad/code/kicad.git"; \
      echo "with these patches applied in this order (sha256):"; cat /src/patches.sha256; \
      echo "Built with:"; \
      grep -E '^(CMAKE_BUILD_TYPE|KICAD_[A-Z_0-9]+):' build/CMakeCache.txt; } > $S/SOURCE.txt; \
    cp kicad/LICENSE* kicad/AUTHORS.txt $S/

# -------------------------------------------------------------- runtime ----
FROM base AS runtime
ARG RUNTIME_PKGS
ARG PREFIX=/opt/kicad-altium
RUN pacman -Syu --noconfirm --needed ${RUNTIME_PKGS} \
    && pacman -Scc --noconfirm \
    && rm -rf /var/lib/pacman/sync/*
# Altium boards use Arial for TrueType text; Liberation Sans has its metrics.
RUN pacman -Sy --noconfirm --needed ttf-liberation \
    && pacman -Scc --noconfirm && rm -rf /var/lib/pacman/sync/*

COPY --from=build /install/ /
# Hand-installed binaries keep their build-tree RPATH; point the loader at lib/.
RUN echo ${PREFIX}/lib > /etc/ld.so.conf.d/kicad-altium.conf && ldconfig
COPY altium2kicad /usr/local/bin/altium2kicad
COPY LICENSE README.md /usr/share/doc/altium2kicad/

ENV PATH=${PREFIX}/bin:${PATH} \
    KICAD_CLI=${PREFIX}/bin/kicad-cli \
    HOME=/tmp/home \
    XDG_CONFIG_HOME=/tmp/home/.config

# Fail the build if any library is missing.
RUN ! ldd ${PREFIX}/bin/kicad-cli ${PREFIX}/bin/*.kiface | grep 'not found' \
    && mkdir -p /tmp/home && chmod 1777 /tmp/home \
    && kicad-cli version

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/altium2kicad"]
CMD ["--help"]
