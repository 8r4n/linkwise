#!/bin/bash
# Build disposable Rocky ARM64 mail test binaries outside the assessment guest.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
out=${1:?"existing absolute output directory"}
[[ $out = /* && -d $out ]]
docker run --rm --platform linux/arm64 \
    -v "$repo:/src:ro" -v "$out:/out" -w /src linkwise-rocky9:builder bash -ec '
    export PATH=/root/.cargo/bin:$PATH RUSTUP_TOOLCHAIN=1.100.0-aarch64-unknown-linux-gnu
    export CARGO_TARGET_DIR=/tmp/target CARGO_PROFILE_RELEASE_DEBUG=0
    cargo build --locked --release --manifest-path rust/Cargo.toml --features mail-tls --bins
    install -m 0755 /tmp/target/release/linkwise /tmp/target/release/linkwise-modemd /out/
'
