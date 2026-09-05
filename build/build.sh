#!/usr/bin/env sh
# The pinned build (build/Dockerfile): the module and its digest in dist/.
#
#   build/build.sh
#
# Requires Docker with BuildKit. The digest printed is the value of the `x`
# tag of the Rule System event that names this build (kind 3417).
set -eu
cd "$(dirname "$0")/.."
rm -rf dist
DOCKER_BUILDKIT=1 docker build \
  --file build/Dockerfile \
  --target artifact \
  --output type=local,dest=dist \
  .
printf 'module: dist/sanki.wasm\ndigest: %s\n' "$(cat dist/digest.txt)"
