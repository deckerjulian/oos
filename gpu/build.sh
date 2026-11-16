#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build.sh - the GPU part of the image (System > Graphics = GPU, gpu/README.md), in Docker, into rootfs/out/gpu/:
#   bin/oos-gpu                       the helper (gpu/oos-gpu.c)
#   usr/lib/aarch64-linux-gnu/...     Mesa with only Panfrost (EGL, GLES 3, GBM), no LLVM at run time, and what it
#                                     links (libdrm, zlib, libstdc++, glibc)
#   lib/ld-linux-aarch64.so.1         glibc's loader (oos-ui itself stays static)
#   etc/licenses/                     the licences of all of it
# boot/build-dev-image.sh runs it when the helper is missing or its stamp differs (rootfs/deps.sh: this script,
# oos-gpu.c, the Mesa line of rootfs/deps.lock). Mesa's build (about 10 minutes) is kept in rootfs/out/mesa, stamped
# with its deps.lock line, so a new Mesa version is built again; remove the directory after a change of its build
# options.
#
# Mesa's own compilers for its built-in GPU programs need LLVM at build time (two stages); the libraries do not.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out="$root/rootfs/out"
DEPS_LOCK="$root/rootfs/deps.lock"
. "$root/rootfs/deps.sh"
ver=$(dep_ver mesa)
image=oos-gpu-build:ubuntu
mkdir -p "$out"
# arm64 whatever the host is (on x86-64 through QEMU, as the rest of the image): the helper and the libraries run on
# the scope. An image of another architecture under this name is built again.
[ "$(docker image inspect -f '{{.Architecture}}' "$image" 2>/dev/null)" = arm64 ] || docker build --platform linux/arm64 -t $image - >&2 <<'EOF'
FROM ubuntu:26.04
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates dpkg-dev \
      meson ninja-build gcc g++ pkg-config python3-mako python3-yaml python3-packaging python3-ply bison flex glslang-tools \
      libdrm-dev libexpat1-dev zlib1g-dev file xz-utils libegl-dev libgles-dev \
      llvm-21-dev libclang-21-dev clang-21 libclc-21-dev llvm-spirv-21 libllvmspirvlib-21-dev \
      spirv-tools spirv-tools-dev spirv-headers libclang-cpp21-dev \
    && rm -rf /var/lib/apt/lists/*
EOF
# the source: the Mesa release, checked against rootfs/deps.lock
dep_fetch mesa "$out" >/dev/null
# a Mesa built from another deps.lock line is built again
if dep_stale "$out/mesa" mesa; then rm -rf "$out/mesa" "$out/mesa.stamp"; fi
rm -rf "$out/gpu"
mkdir -p "$out/gpu"
st=0
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" -v "$here:/g:ro" $image sh -euc '
	W=/w; mkdir -p $W; cd $W
	L=usr/lib/aarch64-linux-gnu
	M=/out/mesa
	if [ ! -f $M/$L/libEGL.so.1 ]; then
	export PATH="/usr/lib/llvm-21/bin:$PATH"
	tar -xJf /out/mesa-$VER.tar.xz
	S=$W/mesa-$VER
	common="--buildtype=release -Db_ndebug=true -Dstrip=true -Dvulkan-drivers= -Dplatforms= -Dgles1=disabled -Dgles2=enabled
	  -Dopengl=false -Dglx=disabled -Dglvnd=disabled -Dvalgrind=disabled -Dlibunwind=disabled -Dzstd=disabled
	  -Dxmlconfig=disabled -Dgallium-va=disabled -Dvideo-codecs= -Dtools= -Dbuild-tests=false -Dgallium-drivers=panfrost"
	meson setup $W/tools $S --prefix=$W/tool-inst $common -Degl=disabled -Dgbm=disabled \
	  -Dllvm=enabled -Dmesa-clc=enabled -Dinstall-mesa-clc=true -Dprecomp-compiler=enabled -Dinstall-precomp-compiler=true \
	  > $W/meson-tools.log 2>&1 || { tail -30 $W/meson-tools.log; exit 1; }
	ninja -C $W/tools install > $W/ninja-tools.log 2>&1 || { tail -30 $W/ninja-tools.log; exit 1; }
	export PATH="$W/tool-inst/bin:$PATH"
	meson setup $W/build $S --prefix=/usr --libdir=lib/aarch64-linux-gnu $common -Degl=enabled -Dgbm=enabled \
	  -Dllvm=disabled -Dmesa-clc=system -Dprecomp-compiler=system > $W/meson.log 2>&1 || { tail -30 $W/meson.log; exit 1; }
	ninja -C $W/build > $W/ninja.log 2>&1 || { tail -30 $W/ninja.log; exit 1; }
	rm -rf $M; mkdir -p $M
	DESTDIR=$M ninja -C $W/build install > /dev/null
	cp $S/docs/license.rst $M/license.rst
	fi
	R=/out/gpu
	mkdir -p $R/bin $R/lib $R/$L $R/etc/licenses
	gcc -std=gnu11 -O2 -Wall -Wextra -I$M/usr/include -o $R/bin/oos-gpu /g/oos-gpu.c -L$M/$L -lEGL -lGLESv2 -Wl,-rpath-link,$M/$L
	strip $R/bin/oos-gpu
	cp -a $M/$L/libEGL.so.1* $M/$L/libGLESv2.so.2* $M/$L/libgbm.so.1* $M/$L/libgallium-*.so $R/$L/
	# what Mesa and the helper link from the system, with glibc and its loader
	for f in $R/bin/oos-gpu $R/$L/*.so*; do LD_LIBRARY_PATH=$R/$L ldd "$f" 2>/dev/null; done | awk "/=> \// {print \$3}" | sort -u | while read -r l; do
		case "$l" in /out/*) ;; *) cp -L "$l" $R/$L/ ;; esac
	done
	cp -L /lib/ld-linux-aarch64.so.1 $R/lib/
	cp $M/license.rst $R/etc/licenses/mesa
	cp /usr/share/doc/libc6/copyright $R/etc/licenses/glibc
	cp /usr/share/doc/libstdc++6/copyright $R/etc/licenses/libstdc++
	cp /usr/share/doc/libdrm2/copyright $R/etc/licenses/libdrm
	cp /usr/share/doc/zlib1g/copyright $R/etc/licenses/zlib
	chown -R "$OUT_OWNER" $R
	du -sh $R; ls $R/$L
' || st=$?
# a finished Mesa keeps its stamp even when the helper failed, so the next try does not build Mesa again
[ ! -f "$out/mesa/usr/lib/aarch64-linux-gnu/libEGL.so.1" ] || dep_stamp "$out/mesa" mesa
[ "$st" = 0 ] || exit "$st"
dep_stamp "$out/gpu/bin/oos-gpu" "$here/build.sh" "$here/oos-gpu.c" mesa
