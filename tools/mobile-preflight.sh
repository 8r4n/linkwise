#!/bin/sh
# Report missing prerequisites for the Linkwise mobile development bench.
set -eu
failed=0
check() {
    label=$1
    shift
    if "$@" >/dev/null 2>&1; then
        printf 'OK      %s\n' "$label"
    else
        printf 'MISSING %s\n' "$label"
        failed=1
    fi
}

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-"$HOME/Library/Android/sdk"}}
rustup_bin=$(command -v rustup || printf '%s' "$HOME/.cargo/bin/rustup")
check 'full Xcode' xcodebuild -version
check 'iOS Simulator runtime' sh -c 'xcrun simctl list runtimes iOS | grep -E "^iOS .*com\.apple\.CoreSimulator\.SimRuntime\.iOS" | grep -vq unavailable'
check 'Android platform tools' test -x "$sdk/platform-tools/adb"
check 'Android Emulator' test -x "$sdk/emulator/emulator"
check 'Android virtual device' sh -c 'test -n "$("$1" -list-avds)"' sh "$sdk/emulator/emulator"
check 'Android NDK' sh -c 'find "$1/ndk" -name source.properties -print -quit | grep -q .' sh "$sdk"
for target in aarch64-apple-ios-sim aarch64-apple-ios aarch64-linux-android; do
    check "Rust target $target" sh -c '"$1" target list --installed | grep -Fxq "$2"' sh "$rustup_bin" "$target"
done
case ${1-} in
    --container)
        check 'GNU Radio container bindings' docker run --rm --network none --pull never \
            linkwise-hf-bench /usr/bin/python3 -c 'from gnuradio import gr'
        ;;
    '')
        check 'GNU Radio Python bindings' "${LINKWISE_GNURADIO_PYTHON:-python3}" -c 'from gnuradio import gr'
        ;;
    *)
        printf 'Usage: %s [--container]\n' "$0" >&2
        exit 2
        ;;
esac
printf '\nWorkspace disk capacity\n'
df -h .
exit "$failed"
