#!/bin/bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
revision=${1:?"Git revision"}
work=${2:?"absolute evidence directory"}
profile=${3:-server}
case "$profile" in
    server) rpm_flags=--mail-tls ;;
    assessment) rpm_flags= ;;
    *) echo 'Expected server or assessment profile' >&2; exit 2 ;;
esac
source="$work/build-source"
mkdir "$source"
git -C "$repo" archive "$revision" | tar -xf - -C "$source"
git -C "$repo" rev-parse "$revision^{commit}" > "$work/build-revision.txt"
image=rockylinux@sha256:d7be1c094cc5845ee815d4632fe377514ee6ebcf8efaed6892889657e5ddaaa6
docker run --rm --interactive --platform linux/arm64 -v "$source:/src" \
    -e "LINKWISE_RPM_FLAGS=$rpm_flags" \
    -v "$here/signing-check.sh:/signing-check.sh:ro" -w /src "$image" bash -se <<'GUEST' 2>&1 | tee "$work/rocky-rpm-build.log"
dnf -y install curl-minimal gcc gcc-c++ make python3 pkgconf-pkg-config alsa-lib-devel openssl-devel \
    rpm-build rpm-sign gnupg2 checkpolicy policycoreutils-devel selinux-policy-devel
export RUSTUP_TOOLCHAIN=1.100.0-aarch64-unknown-linux-gnu
curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUSTUP_TOOLCHAIN" --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version
flags=()
if [[ -n $LINKWISE_RPM_FLAGS ]]; then flags+=("$LINKWISE_RPM_FLAGS"); fi
python3 tools/package-modem.py rpm "${flags[@]}" --output-dir installer-artifacts
rpm=$(find installer-artifacts -maxdepth 1 -name '*.rpm' -print -quit)
sha256sum "$rpm"
python3 tools/smoke-test-package.py "$rpm"
bash /signing-check.sh
GUEST
