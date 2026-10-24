#!/usr/bin/env bash
# Regenerate src/search/gpu/cuda/kernel.ptx from kernel.cu.
#
# Needs clang 18 (on Ubuntu 24.04: sudo apt install clang-18), not the CUDA
# toolkit. Other clang versions work too but produce slightly different PTX;
# CI checks the committed file with clang 18.
#
#   scripts/build-cuda-kernel.sh            rewrite kernel.ptx
#   scripts/build-cuda-kernel.sh --check    only check that it is up to date
set -euo pipefail
cd "$(dirname "$0")/.."

dir=src/search/gpu/cuda
out=$(mktemp)
trap 'rm -f "$out"' EXIT
# sm_50 (GeForce GTX 750 and newer): the driver compiles the PTX for the
# actual GPU when the program starts.
"${CLANG:-clang-18}" -x cuda --cuda-device-only --cuda-gpu-arch=sm_50 -nocudainc -nocudalib \
  -O3 -std=c++20 -Wall -Wextra -Werror -S -o "$out" "$dir/kernel.cu"

if [ "${1:-}" = --check ]; then
  if ! cmp -s "$out" "$dir/kernel.ptx"; then
    echo "$dir/kernel.ptx does not match kernel.cu: run scripts/build-cuda-kernel.sh" >&2
    exit 1
  fi
else
  cp "$out" "$dir/kernel.ptx"
fi
