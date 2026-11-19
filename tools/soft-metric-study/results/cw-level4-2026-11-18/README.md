# BW500 level-4 CW recovery study, 18 November 2026

This exploratory bench study extends the interference work in CW-01 to the level-4 DATA found in archived mixed VARA sessions. It compares two Linkwise receive paths on the same retained stock transmissions. The stock transmitter's host data and the independently parsed clean wire bytes are the oracles. A live stock comparison was blocked when the UTM guest agent became unavailable.

The candidate retries a failed level-4 decode after subtracting one coherent tone. It estimates a line in 1200–1800 Hz and checks frequency agreement, prominence, power fraction, and phase consistency before subtracting it. The receive path is a Python research prototype. The Rust replay helper uses the existing level-4 waveform and host DATA parser. Both methods receive the same coarse burst range. Exact symbol timing and injected tone frequency are withheld from candidate estimation. A clean reference interval sets impairment power. This is a framed test of targeted stationary interference.

The first ten archived frames, from one older successful stock-sender session, form the development corpus. Three later frames from two separate successful stock sessions form the later corpus. The later corpus had three distinct expected host payloads. No parameter changed after development. Trial realizations within a source share a transmission and are dependent.

| Panel | Production baseline | Cancellation prototype |
| --- | ---: | ---: |
| Development CW and noise, pooled | 79/270 | 270/270 |
| Later CW and noise, pooled | 79/270 | 270/270 |
| Later 1309 Hz, pooled over three SIRs | 19/90 | 90/90 |
| Later 1453 Hz, pooled over three SIRs | 60/90 | 90/90 |
| Later 1661 Hz, pooled over three SIRs | 0/90 | 90/90 |
| Later 1453 Hz, 0 dB SIR and 0 dB added-noise ratio | 30/30 | 30/30 |
| Later clean and AWGN controls | 63/63 | 63/63 |
| CRC-valid bodies in 100 noise+CW windows and 200 unencoded-DATA windows | 0/300 | 0/300 |

All CW cells use 0 dB recorded-burst/added-noise ratio with 300–3300 Hz band-limited Gaussian noise. SIR is wanted burst power divided by injected tone power over the same interval. Tone frequencies were 1309, 1453 and 1661 Hz at −12, −6 and 0 dB SIR. Clean bytes and noise model are controlled at the audio input. The outcome does not establish absolute RF sensitivity, an RF false-frame rate, or session-level improvement. The later three stock frames and three modeled tones bound the result.

The negative panel uses 100 seeded noise+CW windows, followed by 100 random independently modulated level-4 DATA-cell waveforms with the normal pilot transitions, replayed clean and with CW plus noise. The generator does not encode a valid FEC frame for the negatives. Its positive control reproduces an archived stock CRC body and exact host bytes. The negative results cover only these constructions and are below the planned ten hours of additional qualification exposure. The level-4 retry still uses the existing CRC decoder and does not implement the level-2 candidate's stricter turbo acceptance rule.

## Reproduction

Use `evaluate-level4-cw.py`, `replay-level4.rs` and `structured-level4-cw.py` from `tools/soft-metric-study/`. The manifest gives the paths and hashes of the original, locally retained stock PCM. An operator on another host must rebind those paths to matching files. Compile the Rust replay and positive-bit tools with edition 2024 and the archived/rebuilt Linkwise release library using `rustc -O -L dependency=<deps> --extern linkwise=<liblinkwise.rlib>`. Use 3 realizations per cell for `--split development` and 10 for `--split later`. Each method sees identical PCM before the candidate's receiver-side transform. Run `synth-positive-bits.rs` with the exact clean wire hex and output path, then pass its bits and independently expected host bytes to `structured-level4-cw.py`.

`check-stock-oracle.py` runs the optional role-aware host fixture against synthetic command and data sockets. It checks both stock roles, exact binary delivery, reconnect, and rejection of wrong and duplicated bytes. This qualifies the host scorer, not the actual VARA transport. The next live step is to restore the guest agent and run both clean role preflights before calibrating CW injection. Level-2 activation is still a distinct CW-01 prerequisite.
