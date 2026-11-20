# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# docker.sh - sourced by the scripts that build or test in Docker: the base images, the arm64 check, and the images
# OOS builds on top of them. The functions keep their variables to the oos_ prefix: they run in the caller's shell.
#
#   $OOS_ALPINE, $OOS_UBUNTU     the base images, named here only, each with the digest of its index for all
#                                platforms: the tag says which release, the digest which build of it Docker takes
#                                (docs/adr/process/0045; docs/fork/deps.md says how to bump)
#   need_docker_arm64            stop with a hint unless Docker runs and runs arm64 containers
#   oos_image NAME [PLATFORM]    the image whose Dockerfile comes on stdin (@ALPINE@ and @UBUNTU@ stand for the base
#                                images), built unless it is there, as NAME:<hash of the Dockerfile and PLATFORM>;
#                                prints that tag. An edit of the Dockerfile is a new tag, so it is built again
#   initramfs_image              oos_image of the arm64 image the initramfs is put together in (its busybox goes into
#                                the image; the sims run the same busybox)
OOS_ALPINE=alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f
OOS_UBUNTU=ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db

need_docker_arm64() {
	docker info >/dev/null 2>&1 || {
		echo "Docker is not running (macOS: open -a Docker; Linux: systemctl start docker, and be in the docker group)" >&2
		exit 1
	}
	# on other hosts arm64 containers need QEMU's binfmt
	docker run --rm --platform linux/arm64 "$OOS_ALPINE" true >/dev/null 2>&1 || {
		echo "Docker cannot run arm64 containers. On an x86-64 host, once: docker run --privileged --rm tonistiigi/binfmt --install arm64" >&2
		echo "(or the distribution's qemu-user-static with binfmt-support)" >&2
		exit 1
	}
}

oos_image() {
	oos_df=$(sed -e "s|@ALPINE@|$OOS_ALPINE|g" -e "s|@UBUNTU@|$OOS_UBUNTU|g")
	oos_h=$(printf '%s\n%s\n' "${2:-}" "$oos_df" | { shasum -a 256 2>/dev/null || sha256sum; } | cut -c1-12)
	oos_tag="$1:$oos_h"
	if ! docker image inspect "$oos_tag" >/dev/null 2>&1; then
		printf '%s\n' "$oos_df" | docker build ${2:+--platform "$2"} -t "$oos_tag" - >&2 || return 1
	fi
	echo "$oos_tag"
}

initramfs_image() {
	oos_image oos-initramfs linux/arm64 <<'DOCKERFILE'
FROM @UBUNTU@
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      busybox-static cpio gzip device-tree-compiler && rm -rf /var/lib/apt/lists/*
DOCKERFILE
}
