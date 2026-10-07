#!/usr/bin/env bash
# Builds both web variants and the auto-selecting loader into dist/.
#   dist/gpu/  WebGPU, high quality      dist/gl/  WebGL2 fallback
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
trunk build --release --dist dist/gl --public-url ./ index.html
trunk build --release --dist dist/gpu --public-url ./ index-gpu.html
cp web/loader.html dist/index.html
echo "built dist/gl, dist/gpu and dist/index.html"
