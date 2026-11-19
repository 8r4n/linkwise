# KiwiSDR passive decoder comparison, 18 November 2026

Capture attempts ran from 15:46:01 to 15:54:58 PST. Receivers stayed in receive-only mode on the VarAC calling frequencies of 7.105 and 14.105 MHz USB. The pinned KiwiClient revision is in the manifest. Receipt counts below count each receiver recording separately.

| Capture | Dial (MHz USB) | Audio (s) | Baseline | Candidate | Disposition |
| --- | ---: | ---: | ---: | ---: | --- |
| free-union-14105 | 14.105 | 0.00 | — | — | No audio returned |
| kfs-omni-14105 | 14.105 | 0.00 | — | — | No audio returned |
| kitchener-7105 | 7.105 | 180.07 | 2 | 2 | Full window |
| kitchener-7105-repeat | 7.105 | 180.07 | 4 | 4 | Full window |
| newton-14105 | 14.105 | 0.00 | — | — | No audio returned |
| rockport-7105 | 7.105 | 9.52 | 0 | 0 | Short window |
| san-diego-14105 | 14.105 | 0.00 | — | — | No audio returned |
| warrenton-7105 | 7.105 | 180.04 | 2 | 2 | Full window |
| warrenton-7105-repeat | 7.105 | 180.08 | 4 | 4 | Full window |
| westminster-7105 | 7.105 | 0.00 | — | — | timed out |

Both decoders produced 12 identical CRC-valid receptions across 8 distinct baseline KISS payload hashes, with 0 baseline losses and 0 additional candidate frames. The fallback evaluated 113 failed hypotheses and activated 0 times. This batch verifies preservation on the observed frames. Opportunities to measure a gain require natural signals that activate the candidate. There were 4 provisional paired-receiver matches with the same wire and KISS hashes. Their timing is estimated from packet arrivals and sample indices.

The archive includes original audio hashes, durations, sequence continuity, overload flags, and both full replay results by hash. Original audio stays in the local capture folder recorded in the provenance file. The two decoders consumed identical 48 kHz PCM derived with `afconvert`. No noise or other impairment was added. The full recordings were submitted through stdin to avoid a duplicate float32 file. The replay entry point changed only its input loader. The frozen research DSP modules were byte-identical to the prior acceptance prototype. As a pipeline check, both paths recovered all 14 receptions from the earlier archived two-receiver corpus. The two replay wall times include process startup and pipe transfer.

This receive-only run provides RF observations and a paired decoder regression on the captured audio. It does not supply a transmitter-side count, an independent exact-payload oracle for every event, absolute RF SNR, or a stock VARA replay. Controlled K1–K4 and held-out VOACAP qualification remain separate. The 20 m connection failures do not establish the absence of transmissions on that band.
