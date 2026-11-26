#!/bin/bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
revision=${1:?"Git revision"}
work=${2:?"absolute evidence directory"}
profile=${3:-server}
container_runtime=${LINKWISE_CONTAINER_RUNTIME:-docker}
case "$profile" in
    server) rpm_flags=--mail-tls ;;
    assessment) rpm_flags= ;;
    *) echo 'Expected server or assessment profile' >&2; exit 2 ;;
esac
if [[ $work != /* || ! -d $work ]]; then
    echo 'Provide an existing absolute evidence directory.' >&2
    exit 2
fi
if ! command -v "$container_runtime" >/dev/null; then
    printf 'Container runtime unavailable: %s\n' "$container_runtime" >&2
    exit 127
fi
commit=$(git -C "$repo" rev-parse --verify --end-of-options "$revision^{commit}")
source="$work/build-source"
mkdir "$source"
git -C "$repo" archive "$commit" | tar -xf - -C "$source"
printf '%s\n' "$commit" > "$work/build-revision.txt"
signing_fixture="$work/rocky-signing-check.sh"
cp "$here/signing-check.sh" "$signing_fixture"
image=rockylinux@sha256:d7be1c094cc5845ee815d4632fe377514ee6ebcf8efaed6892889657e5ddaaa6
"$container_runtime" run --rm --interactive --platform linux/arm64 -v "$source:/src:Z" \
    -e "LINKWISE_RPM_FLAGS=$rpm_flags" \
    -e "LINKWISE_BUILD_IMAGE=$image" \
    -v "$signing_fixture:/signing-check.sh:ro,Z" -w /src "$image" bash -se <<'GUEST' 2>&1 | tee "$work/rocky-rpm-build.log"
printf 'Pinned builder: %s\n' "$LINKWISE_BUILD_IMAGE"
cat /etc/rocky-release
uname -m
dnf -y install curl-minimal gcc gcc-c++ make python3 pkgconf-pkg-config alsa-lib-devel openssl-devel \
    rpm-build rpm-sign gnupg2 checkpolicy policycoreutils-devel selinux-policy-devel \
    systemd-rpm-macros sudo
export RUSTUP_TOOLCHAIN=1.100.0-aarch64-unknown-linux-gnu
curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUSTUP_TOOLCHAIN" --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version
flags=()
if [[ -n $LINKWISE_RPM_FLAGS ]]; then flags+=("$LINKWISE_RPM_FLAGS"); fi
python3 tools/package-modem.py rpm "${flags[@]}" --output-dir installer-artifacts
rpm=$(find installer-artifacts -maxdepth 1 -name '*.rpm' -print -quit)
sha256sum "$rpm"
if [[ $LINKWISE_RPM_FLAGS == --mail-tls ]]; then
    BW500_PROFILE=$(python3 tools/fetch-modem-profile.py)
    sha256sum "$BW500_PROFILE"
    python3 tools/smoke-test-package.py "$rpm" --rocky-bw500-profile "$BW500_PROFILE"
else
    python3 tools/smoke-test-package.py "$rpm"
fi
bash /signing-check.sh
GUEST
