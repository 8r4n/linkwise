# Linkwise browser modem

Build the worker module from the repository root.

```sh
rustup target add wasm32-unknown-unknown
cargo build --manifest-path rust/hf-browser/Cargo.toml \
  --target wasm32-unknown-unknown --release --locked
node rust/hf-browser/tests/check.mjs \
  rust/hf-browser/target/wasm32-unknown-unknown/release/linkwise_hf_browser.wasm
```

The build uses the canonical Linkwise codec sources. The browser uses isolated
DATA frames. BW500 runs the native level-1 pilot receiver and carries nine
payload bytes. BW2300 and BW2750 use the native level-4 pilot and CRC decoder and
carry up to 86 bytes in a closing DATA frame. Their training is initialized
with a fixed simulation callsign. Session setup, ARQ, radio control and RF
interoperability are outside this module.

The channel uses two equal-mean Gaussian-scatter paths with 1 ms delay and
0.5 Hz Doppler spread, defined as two standard deviations. These are the
mid-latitude moderate parameters in ITU-R F.1487. The receiver noise reference
is a 300–3300 Hz Hamming FIR. The Rust channel and Python bench use separate
random number generators and can produce different waveforms for the same
seed. These tests check the browser technique and frame behavior. Formal
F.1487 comparative testing requires a separate BER qualification.

## Worker interface

One worker owns the instance. `lw_sample_rate()` returns 48000 and
`lw_max_samples()` returns the capacity of each 32-bit float slot.
`lw_buffer_ptr(slot)` exposes a byte address for a writable slot.
Slot 0 stages transmit or decoder-input samples, slot 1 holds the propagated
signal, and slot 2 holds the independent noise. An invalid slot returns zero.
Use each current `memory.buffer` when making JavaScript typed-array views
after a WASM call. A call can grow memory. Copy data needed for later use
before staging a new burst in the same slot.

`lw_set_mode(bandwidth)` accepts 500, 2300, or 2750 and returns 1 on success.
Unsupported values return 0 and keep the prior mode. `lw_payload_ptr()`
exposes the common byte mailbox. `lw_payload_capacity()` gives the selected
mode's usable bytes. `lw_encode()` encodes nine bytes. `lw_encode_len(count)`
selects an explicit length. Use exactly nine bytes for BW500, padding at the
application layer if needed. Use 1–86 bytes for a wide mode. Encoding returns
the number of transmit samples in slot 0, or zero on invalid input.

`lw_propagate(count, noise_seed, path_seed, start_sample, snr_db, include_noise)`
uses the exact staged waveform and leaves slot 0 intact. Specify an integer
absolute sample position as a JavaScript number. Use a consistent unsigned
path seed for one source, receiver and band to keep fading continuous. Use a
separate attempt seed for noise. The function writes `count + 48` samples per
output, aligned to the transmit start, and returns that length or zero on
invalid input. The 48-sample tail belongs to the receiver. With
`include_noise` set to 0, the corresponding noise slot contains zeros.
`lw_reference_power(slot, count)` measures mean square through the receiver
reference filter and returns NaN for invalid input.

Stage the complete receiver waveform, including any aligned interferers, in
slot 0. `lw_decode(count)` returns 1 for one valid frame, 0 for failure to
recover one frame, and −1 for invalid audio or length. On success
`lw_decoded_len()` gives the actual payload length and the mailbox contains
the recovered bytes. Apply the application's end-to-end identity or payload
check separately.
