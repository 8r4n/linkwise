#!/bin/sh
# One bounded, explicitly configured Kiwi audio capture. No radio/PTT commands.
set -eu
: "${KIWI_RUN_DIR:?set the experiment directory}"
: "${KIWI_CLIENT_DIR:?set the pinned KiwiClient checkout}"
: "${KIWI_PY:?set the capture environment Python executable}"
: "${KIWI_HOST:?set the agreed receiver hostname}"
: "${KIWI_PORT:?set the receiver port}"
: "${KIWI_RF_KHZ:?set the station-approved USB carrier frequency in kHz}"
: "${KIWI_LOW_HZ:?set the verified audio passband lower edge}"
: "${KIWI_HIGH_HZ:?set the verified audio passband upper edge}"
: "${KIWI_GAIN_DB:?set the documented receiver fixed gain}"
: "${KIWI_CAPTURE_S:?set the agreed duration in seconds}"
: "${KIWI_NODE_ID:?set the candidate ID for this capture}"
case "$KIWI_NODE_ID" in
    R[0-9][0-9]) ;;
    *) echo "invalid receiver ID" >&2; exit 2 ;;
esac
expected_revision=4eb733e6b6147f7fbeb97ced64cdac029b202d18
actual_revision=$(git -C "$KIWI_CLIENT_DIR" rev-parse HEAD)
if [ "$actual_revision" != "$expected_revision" ]; then
    echo "KiwiClient revision differs from the reviewed command profile" >&2
    exit 2
fi
capture_dir="$KIWI_RUN_DIR/$KIWI_NODE_ID/original"
if [ -e "$capture_dir" ]; then
    echo "Capture directory already exists; preserve it and choose a new run" >&2
    exit 2
fi
"$KIWI_PY" -c 'import sys; t,f,lo,hi,g=map(float,sys.argv[1:]); assert 0<t<=900 and 0<f<32000 and 0<=lo<hi<=6000 and -100<=g<=100, "invalid capture profile"' "$KIWI_CAPTURE_S" "$KIWI_RF_KHZ" "$KIWI_LOW_HZ" "$KIWI_HIGH_HZ" "$KIWI_GAIN_DB"
mkdir -p "$capture_dir"
# subprocess timeout is an independent wall-clock limit, including connection time.
# A timeout/abrupt termination makes affected samples ineligible pending inspection.
"$KIWI_PY" -c 'import subprocess,sys; subprocess.run(sys.argv[2:],timeout=float(sys.argv[1])+30,check=True)' \
    "$KIWI_CAPTURE_S" "$KIWI_PY" "$KIWI_CLIENT_DIR/kiwirecorder.py" \
    -s "$KIWI_HOST" -p "$KIWI_PORT" -f "$KIWI_RF_KHZ" -m usb \
    -L "$KIWI_LOW_HZ" -H "$KIWI_HIGH_HZ" -g "$KIWI_GAIN_DB" \
    --ncomp --tlimit "$KIWI_CAPTURE_S" --connect-timeout 5 --connect-retries 1 \
    --busy-timeout 5 --busy-retries 1 --fn raw --log info -d "$capture_dir" \
    > "$KIWI_RUN_DIR/$KIWI_NODE_ID/capture.log" 2>&1
