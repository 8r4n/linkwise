# VARA HF interoperability bench

The live reference is stock VARA HF 4.9.1 in Windows ARM64 UTM guests on
the MacBook and Mac mini. The tested Windows bridge captures Speakers
loopback and plays the other modem into VB-CABLE through shared WASAPI,
with a 250 ms playback prebuffer. VARA receives from CABLE Output.
Use the existing interactive Windows audio session and isolated bench
routing. Keep radio interfaces outside this fixture.

## Run a case

Start the host runner before launching the corresponding Windows bridges.
The stock caller uses host ports 28760–28762, and the stock listener uses
28770–28772, in command, DATA, and audio order. Each guest connects to its
own host at `10.0.2.2`. When the listener is on the Mac mini, reverse-forward
its three loopback ports over SSH to the MacBook, where the runner listens.
The runner signals the audio bridges after the required sockets connect.
The Windows bridge sources used for these runs are saved alongside the
transcripts in the lab evidence.

From the checkout root:

```sh
cargo build --locked --manifest-path rust/Cargo.toml --bin linkwise-modemd
profile=$(python3 tools/fetch-modem-profile.py)
python3 tools/test-vara-interop.py stock /path/to/new/stock-run
python3 tools/test-vara-interop.py linkwise-caller /path/to/new/mixed-run \
  --modem rust/target/debug/linkwise-modemd --profile "$profile"
```

Use `linkwise-responder` to substitute the listener. Use `linkwise` with
`--modem` and `--profile` for a two-Linkwise baseline using the same
host payloads. Add `--verify-disconnect` to each case for consistent
teardown criteria. The JSON report includes connection, transfer and
disconnect timing, directional payload hashes, and recorded audio duration.
Use `--pace-virtual` to delay delivery by each burst’s sample duration.
Without it, the two-Linkwise relay can process audio faster than real time.
Stock bridge recordings include idle audio. Account for those differences
before comparing timing or recorded duration across routes. Select `--bandwidth
2300` or `--bandwidth 2750` for a wide case. The default payloads are
203 → 6 → 203 bytes at BW500 and 13 → 6 → 13 bytes at wide bandwidths.
Use `--payload-length 203` to exercise wide multi-frame transmit as caller
or `--reply-length 203` to exercise it as responder. A mixed case can
select one locally routed stock bridge with `--stock-bridge-base`.

The runner checks both exact byte streams and retains the command transcript,
raw received bytes, little-endian mono float32 audio and chunk timestamps.
A mixed run also retains the finite Linkwise waveforms and their paced Windows
playback stream. `result.json` records the binary and profile hashes, fixture
outcome, injection status, and which endpoints reported disconnect in the
observation window. A payload pass by itself does not qualify disconnect.
Use `--verify-disconnect` to require the caller to drain its queue and
both modems to report disconnect.
Use `--disconnect-from responder` to initiate teardown from the listener.
`--verify-reconnect` additionally requires fresh DATA on the same sockets
and a second two-ended disconnect.
For the BW500 late-write case, use `--payload-length 200
--late-caller-write-delay 1 --verify-disconnect`. This sends two more
bytes from the caller one second after `BUFFER 0`. Set the delay to
`-1` to queue the continuation after the peer receives the first
delivery and before the caller processes the final ACK.
Retain the Windows bridge logs and stereo float32 VB-CABLE taps with the run.
A disconnected bridge, audio fault, or missed injection needs investigation
before counting a run toward sign-off.

## Impairment fixtures

For the GNU Radio HF comparison, use `--hf-snr 32` or `--hf-snr 22`
with `--hf-worker` pointing to the framed-PCM worker from the lab
evidence. Build the image with
`docker build -t linkwise-hf-bench containers/hf-bench`. Both directions
use the same profile and independently initialized channel state. The
32 dB profile uses a 1 ms echo at −9 dB, 0.5 Hz Doppler spread and seed 17.
The 22 dB profile uses a 2 ms echo at −4 dB, 1 Hz Doppler spread and seed 18.
Both use Rician K = 10 and set noise from the input RMS of each burst.
Use `--pace-virtual` for the two-Linkwise case. Stock audio is captured
as complete bursts before applying the channel, then played at real time
into a stock receiver. This adds relay delay to stock routes. Transfer
times include the relay and the final transmit queue drain.


- `--replace-burst ORIGINAL_F32 REPLACEMENT_WAV` replaces one Linkwise
  burst guarded by the SHA-256 of the original audio. Use the 200-byte BW500
  fixture with an invalid-CRC first DATA burst of identical duration. The
  saved source and replacement remain available for replay.
- `--corrupt-stock-first REPLACEMENT_WAV` injects the invalid CRC on the
  first full-length stock caller DATA burst while Linkwise is the responder.
  The original stock waveform and the substituted waveform are retained.
- `--corrupt-stock-after-prefix REPLACEMENT_WAV` waits for the stock caller
  to report 157 bytes buffered in the 200-byte fixture before injecting.
- `--raise-stock-rate ACK_F32 INCREASE_F32` can accompany the preceding case.
  After observing the first level-3 retry, it replaces one hash-matched
  Linkwise acknowledgment with the captured stock rate-increase reply.
- `--drop-stock-ack intermediate` and `--drop-stock-ack final` withhold one
  reply in the 200-byte Linkwise-caller case. The guard uses the number of
  bytes the stock host received and the burst duration. Decode the saved
  `dropped.f32` before claiming that the intended reply was withheld.
- `--drop-linkwise-ack intermediate` and `--drop-linkwise-ack final`
  withhold one generated ACK in the 200-byte BW500 Linkwise-responder case.

For the BW500 rate-increase socket replay, pass the captured W1A reply to
`tools/test-virtual-bw500.py --stock-rate-increase` along with the usual
modem and profile arguments. Use `--stock-intermediate-query-ack` with
the captured response to the short intermediate query,
`--stock-query-missing-data` with the captured keyed missing-DATA reply, and
`--stock-wide-nak` with the captured BW2750 NAK to check a level-3
retry of the outstanding closing frame.
Pair `--stock-turn-release` with `--stock-short-turn-request`
to replay the two caller controls captured in the BW500 Pat
stock-to-stock run. Use `--stock-crossed-turn` with the captured caller
turn request and idle frame, a called-station-keyed drained control, and
the captured caller DATA rejection to replay the responder's queued
continuation and lower-rate retry.

## Initial bench results

| Case | Result |
| --- | --- |
| Stock BW500 203 → 6 → 203 | Three consecutive exact exchanges on the streaming route. |
| Stock BW2750 203 → 6 → 203 | Exact on the two-Mac route, including rate changes between levels 3 and 4. Both endpoints reported orderly disconnect after the caller drained. |
| Mixed BW500 203 → 6 → 203 | Both roles passed. Linkwise's responder handback was verified against the stock caller. |
| BW500 first-frame CRC rejection, 200 → 6 → 200 | Linkwise as sender passed three live runs. Stock as sender passed once. Stock accepted Linkwise's level-3 retry and requested a return to level 4 after the first 34 bytes. |
| BW500 corruption after an acknowledged prefix | Linkwise as sender passed three consecutive runs on one binary. Stock as sender passed once. The Linkwise retry retained the first 43 bytes, sent 34 at level 3, and sent the remainder at level 4. |
| BW500 stock intermediate and final ACK withheld | Three exact exchanges per position across two Linkwise builds. The intermediate sequence included one missed injection, excluded from the pass count. All six counted captures decoded as the intended keyed ACK. |
| BW500 Linkwise intermediate and final ACK withheld | One exact exchange per position. The intermediate case needed a keyed query response while receiving. A separate run recovered forward DATA before the reverse DATA overlapped a stock turn reply. |
| BW2300 and BW2750 13 → 6 → 13 | Passed with either modem as caller. The captured turn releases use the negotiated wide-mode tone bins. |
| BW2300 and BW2750 stock-to-Linkwise 203-byte DATA | Each passed after enabling intermediate frame reception and matching the stock wide-mode ACK. Stock stayed at level 4 in both mixed runs. |
| Wide level-4 multi-frame transmit | BW2300 and BW2750 each passed with Linkwise transmitting 203 bytes as caller and as responder. Each 203-byte delivery used 89 + 89 + 25-byte frames. BW2750 previously hit a closing-frame NAK on the second delivery. |
| BW2750 rejection after an acknowledged prefix | Captured-NAK replay verified that Linkwise retransmits only the outstanding frame. Both a stronger impairment and an unmodified live run produced a keyed NAK after 89 bytes. Linkwise replayed the outstanding frame. Stock did not acknowledge it before timeout. |
| Host socket loss and reconnect | The virtual socket regression dropped command, DATA, and audio individually. Each fresh session delivered its own payload exactly without stale DATA. |
| Mixed orderly disconnect | BW2750 delivered 13 + 6 + 13 bytes and drained. Linkwise reported a host-side disconnect, and the stock peer remained connected through the 15-second observation window. Both peer reports are required to qualify the wire exchange. |
| Pat B2F, stock VARA to stock VARA | Passed on the two-Mac BW500 route. The receiver stored the message and the extracted 24-byte attachment matched the original SHA-256. The stock caller resumed sending after its queue reached six bytes during a turn exchange. |
| Pat B2F, stock VARA to Linkwise | The turn fix cleared the proposal checksum. Capture replay verified both observed rate-down frame replacements. In the final live run, the attachment recovered from audio matched the original SHA-256, but the two-byte B2F end marker stayed in the stock queue after an unanswered `Bw500Drained`. The recipient mailbox remained empty. |
| Pat B2F, Linkwise to stock VARA | The initial two-Mac retest received a keyed BW500 setup NAK, retried, and timed out. |

## Completion runs

| Follow-up | Live evidence |
| --- | --- |
| Stock VARA → Linkwise Pat | Passed. A stock control confirmed the poll ACK after a late-write `Bw500Drained`. The mixed recipient stored the message, and the 24-byte attachment matched the source SHA-256. |
| Linkwise → stock VARA Pat | Passed recipient delivery. The 200-byte fixture showed stock retaining level 3 for its late two-byte continuation. Linkwise now retains level 3 for a short caller delivery after a level-3 close. Stock accepted the B2F end marker at level 3, and Pat stored the message with a SHA-256-verified 24-byte attachment. |
| Mixed orderly disconnect | Both Linkwise roles initiated a successful BW2750 disconnect. A Linkwise-initiated BW2750 run and a stock-responder-initiated BW2300 run each delivered an exact fresh probe and disconnected again on the same host sockets. |
| Wide recovery, Linkwise as sender | BW2750 passed after a direct NAK and level-3 retry. BW2300 passed after a state query, 22-byte level-2 retry, and return to level 4. Both recovered all 412 bytes and confirmed disconnect. BW2300 also passed a same-socket reconnect. |
| Wide recovery, Linkwise as receiver | BW2300 and BW2750 passed a forced rejection after the 89-byte prefix. Stock retried at level 3. Each run matched all 412 bytes and passed stock-initiated disconnect and same-socket reconnect. |
| Wide stock controls | BW2300 and BW2750 both passed injected corruption after the acknowledged 89-byte prefix. Each stock sender retried 22 bytes at level 2 and returned to level 4. |

The mixed BW500 late-write fixture delivered all 202 bytes and disconnected
at both ends for writes before the final ACK and one second after
`BUFFER 0`. A stock-to-Linkwise Pat retry also exposed an unpadded
33-byte level-3 close. The new decoder accepted the captured `0x83`
frame and replayed all 393 B2F bytes exactly. A subsequent live
stock-to-Linkwise run stored the message and verified the attachment
SHA-256. In the successful Linkwise-to-stock run, the sender's Pat
process reached its fixture timeout after recipient delivery. A
follow-up Linkwise-to-stock run stalled earlier in the attachment
transfer, so repeat qualification remains open.

The wide level-2 and level-3 allocations come from the two-Mac stock
captures. The transmit rate changes are bounded to the observed layouts
and retain the acknowledged prefix. The disconnect and recovery
experiments also used Saul St John's
[VARA HF control measurements](https://github.com/sstjohn/hfmodem/blob/main/docs/protocols/vara/05-arq-session-state-machine.md)
and [DATA answer recovery measurements](https://github.com/sstjohn/hfmodem/blob/main/docs/protocols/vara/21-data-answer-recovery.md).

## Sign-off

Track each role, bandwidth, payload shape, and impairment separately.
Require three consecutive live passes for a required case, with saved
evidence. Use the stock control after changes to the Windows audio path.
Native audio and RF, Winlink client workflows, and versions other than
VARA HF 4.9.1 have their own qualification results.

The paired Pat captures isolate the BW500 turn exchange when host
DATA arrives late. After the final ACK, the stock responder answered
the caller release with a poll ACK, then granted the following short
caller turn request. Linkwise now uses that sequence with an empty
responder queue. The mixed retest cleared the six-byte `F>`
checksum that had remained queued. The receiver also recognizes
the captured level-4 to level-3 retry layout and replaces the
uncommitted level-4 frame when the new payload matches its prefix.
Qualify both mixed Pat directions and Winlink Express with
verified recipient payloads.

### November 11 sign-off retests

| Check | Evidence |
| --- | --- |
| BW500 missing DATA after an acknowledged prefix | Replayed a keyed short response captured from stock VARA. Linkwise preserved the acknowledged 86 bytes, recovered the remaining 117 at level 3, and delivered all 203 bytes exactly. |
| BW500 Pat attachment and close | Two Linkwise-to-stock runs stored the 24-byte attachment with matching SHA-256. The traced run showed Pat sending `DISCONNECT` with three B2F quit bytes queued, Linkwise replying `WRONG`, and Pat timing out. Linkwise now accepts the command and waits for the queue to drain. The socket regression verifies exact quit bytes, `BUFFER 0`, and `DISCONNECTED` at both ends. A live retest exited Pat with status 0 and verified the exact attachment. The trace stopped after Linkwise reported `DISCONNECTED` and before recording stock's final notification. |
| BW500 stock greeting and setup | One stock greeting carried the CRC-valid level-4 field `0x41`. Replay now accepts that frame and delivers the complete 49-byte greeting with its closing frame. A later setup timeout exposed a stock `0x02` setup control with the same caller, keyed trailer and level request as the accepted `0x82` form. The decoder now accepts both BW500 forms and verifies the CRC and caller identity. |
| BW500 long DATA rejection after a query | Two live traces stalled with six B2F proposal checksum bytes buffered. Stock's 32-tone rejection matched the called-station seed offset 288 and preadvance 63. Replay verifies the exact six-byte payload after retrying at level 3. |
| BW500 mixed Pat | On build `d7d1e04b`, both directions delivered the 24-byte attachment with matching SHA-256, exited Pat with status 0, and logged `DISCONNECTED` at both endpoints in the first retest of each direction. Repeats hit a missing DATA acknowledgment and a stock-side buffer flush timeout. |
| BW500 final Linkwise ACK withheld | Three consecutive live runs on build `5c06d16f` delivered all 406 bytes exactly after injection and confirmed disconnect at both ends. Build `0374619e` timed out twice in reverse delivery after injection. The capture showed a keyed `60/63` DATA rejection after an unaccepted reverse over. Builds `811c25b8` and `6a4dc89e` recognize the rejection and retry at level 3. Each passed two live runs with injection, exact delivery, and two-ended close. Setup timeouts interrupted repeat qualification on both builds. |
| BW500 intermediate Linkwise ACK withheld | Builds `0374619e` and `811c25b8` each passed three consecutive live runs with injection, exact delivery of all 406 bytes, and disconnect at both ends. Build `6a4dc89e` passed once. Its other attempts timed out during setup and before the first delivery completed. |
| Wide recovery, Linkwise as sender | On build `d7d1e04b`, BW2300 passed twice, with setup or turn-grant timeouts interrupting the sequence. On build `6a4dc89e`, BW2300 passed once and BW2750 passed twice with guarded DATA corruption, all 412 bytes exact, two-ended disconnect, and an exact 12-byte same-socket reconnect probe. Stock accepted and rejected the same BW2300 level-2 retry waveform on separate runs. A BW2750 repeat accepted 22 retry bytes and stalled with the final three bytes outstanding. |
| Winlink Express → Pat, mixed P2P | Express 1.8.6.0 used stock VARA HF 4.9.1 at BW500. The Express editor and outbox showed the test message as P2P. Pat through Linkwise stored the message and its 24-byte attachment matched the original SHA-256. |
| Pat → Winlink Express, mixed P2P | Early attempts stalled when the modems requested opposite turns. Recognizing the stock caller's idle frame allowed Linkwise to resume sending. A later capture isolated the final two B2F bytes, which stock did not accept at level 4 after a level-3 close. Build `0d882c77` retained level 3 for the short continuation. Express stored P2P message `SKSJ3GAT6D5P` in the inbox, and the 26-byte attachment matched the source SHA-256. Stock then sent an end-of-proposals frame with control `0x02`. The bench captured it, and the decoder regression verifies the exact `FF\r` payload and rejects a wrong session key. The sender-side close still needs a live retest. |

The retest logs, build hashes, and keyed missing-DATA capture are in the
bench evidence directory under `2026-11-11-vara-to-vara/signoff`.

The frozen build still needs complete repeat sequences for mixed Pat,
the BW500 ACK-loss rows, and wide DATA recovery. One BW500 setup failure
reached a CRC-valid stock setup requesting level 3 with control `0x42`.
Other attempts stayed in stock's control queries before an accepted setup.

### November 12 wide DATA recovery sign-off

On merged main commit `70cf5ed`, all four mixed wide DATA recovery rows
met the three-consecutive-live-pass gate against stock VARA HF 4.9.1.
Each counted trial injected corruption after an acknowledged 89-byte
prefix, observed a lower-rate retry, and delivered the exact 203, 6,
and 203-byte streams without duplication. Both peers reported disconnect.
On the same host sockets, a fresh session delivered an exact 12-byte
probe and both peers disconnected again.

| Bandwidth | Linkwise role in impaired delivery | Consecutive trials | Observed retry |
| --- | --- | --- | --- |
| BW2300 | Caller sending DATA | `tx-2300-02` through `04` | Level 2 |
| BW2300 | Responder receiving DATA | `rx-2300-07` through `09` | Level 3 |
| BW2750 | Caller sending DATA | `tx-2750-03` through `05` | Level 3 |
| BW2750 | Responder receiving DATA | `rx-2750-02` through `04` | Level 3 |

The transmit trials used stock VARA in the MacBook Windows UTM guest.
The receive trials used stock VARA in the Mac mini Windows UTM guest,
with the bridge ports forwarded over the Thunderbolt link. Earlier
attempts with missed injection, no observed fallback, or incomplete
sessions were excluded. One BW2300 receive attempt lost 2,400 samples
from a captured stock turn grant and timed out before reverse delivery.
A separate attempt timed out during setup.

The local evidence directory is
`reference/vara/fixtures/wide-recovery-2026-11-12/`. It contains the modem
hash, per-trial audio and transcripts, fixture inputs, exact received
bytes, and an independently checked summary. The virtual wide suite
and captured BW2750 NAK replay also passed. This sign-off covers
radio-free DATA recovery for these role and bandwidth combinations.

## BW500 receiver sensitivity experiment

This experiment replays stock 48 kHz WAV bursts into Linkwise's bench
acquisition and decoders. Use a tab-separated manifest with two columns, no
header, and one row per fixture. The first column is one of `data1`, `data2`,
`data3`, `data4`, `setup`, `ack`, `request`, or `response`. The second is the
absolute WAV path. Use connected ACKs and requests keyed to W1AW, and
responses keyed to KN6MQS. Use the stock matrix
measurement profile, `harness/profile.json`. Keep the fixture order for
reproducible noise seeds. The study's capture manifest records the original
relative paths and SHA-256 values.

```sh
export BW500_STUDY_INPUT=/path/to/input.tsv
export BW500_STUDY_PROFILE=/path/to/harness/profile.json
export BW500_STUDY_TRIALS=32
export BW500_STUDY_CSV=/path/to/awgn-trials.csv
cargo test --release --locked --manifest-path rust/Cargo.toml --lib \
  stock_awgn_sweep -- --ignored --nocapture
python3 tools/plot-bw500-sensitivity.py "$BW500_STUDY_CSV" /path/to/plots --title AWGN
```

The default grid is −16 through +44 dB in 4 dB steps. The receiver input is
float PCM and excludes clipping. SNR is the active captured burst power
relative to the equivalent AWGN power in 3 kHz. Noise runs through the
lead and trailing guards. For a fixed 2 ms echo at −6.02 dB relative to the
direct path, set `BW500_STUDY_ECHO_SAMPLES=96`. For separate checks set
`BW500_STUDY_GAIN=0.1` for a 20 dB reduction in absolute input level,
`BW500_STUDY_OFFSET_HZ=15` for a carrier offset, or
`BW500_STUDY_INTERFERER_DB=-6` for a continuous 1500 Hz tone at −6 dB
relative to the signal power. Use `BW500_STUDY_MIN_SNR`,
`BW500_STUDY_MAX_SNR`, and `BW500_STUDY_STEP` to adjust the grid.

The CSV distinguishes energy gate starts within 20 ms of the clean start,
CRC-valid exact DATA/setup, and exact keyed ACK matches. Start detection
and noise-only trigger rates are recorded at each gate's own operating
point. The clean-window diagnostic supplies burst boundaries. The aligned
diagnostic also skips DATA/setup pilot acceptance and supplies the clean
symbol epoch. With an injected frequency offset, it removes that known
offset. Keyed controls continue to use their own search inside the supplied window.
The candidate is an offline FFT bandpass plus a threshold estimated from a
known quiet guard, with a separate path feeding the filtered PCM to the
demodulator. The production modem continues to use its current receiver.

For the ACK echo diagnostic, run `stock_ack_echo_diagnostic` instead of
`stock_awgn_sweep` with a distinct output CSV. It reports the mean keyed
pair power and weakest tone at the best timing for the correct key, two
wrong keys, and noise-only controls. Given the clean search window, these
scores can distinguish a weak-tone failure from a missed burst.

For a live mixed BW500 probe, use `--stock-awgn-db 0 --stock-awgn-seed 4001 --stock-awgn-gain 0.2`
with the existing virtual-audio runner and a calibrated streaming bridge.
The runner applies AWGN only to finite stock-to-Linkwise ingress records,
including their existing guards. It derives noise from active window RMS
and logs RMS, noise scale, and the number of records processed. A common
gain preserves SNR and keeps the sum within the daemon's PCM range. This is a
directional session probe. A session curve requires repeated trials of
both directions at each SNR and a stock-to-stock control on the same route.

### Measured BW500 sensitivity on main at ff66de6

The first sweep used 16 stock captures across setup, connected ACK and
DATA levels 1–4. At 0 dB in AWGN, the current bench path recovered
0/512 trials. Band-limiting the gate and decoder input recovered 500/512.
With the clean symbol epoch and pilot acceptance bypassed, DATA levels 1
and 2 recovered 64/64 each at −12 dB. The fixed gate depends on absolute
input level. With the same 24 dB SNR and 20 dB lower input gain the
baseline recovered 128/128, versus 0/512 at the original gain and 24 dB.
The reduced-gain sweep used a subset of the original seeds.

An additional six stock request/response captures were swept at 20% gain.
At 0 dB the baseline and supplied-clean-window paths each recovered 0/96.
The band-limited detector and demodulator path recovered 94/96. The
prototype also reduced level-3 recovery at +44 dB, from 96/96 to 63/96,
so it has a clean-channel regression to resolve before deployment.

A fixed 2 ms echo reduced connected ACK recovery to 0/48 at +44 dB in the
standard and filtered paths. A separate clean-window diagnostic kept
the aggregate tone threshold at 0.8 and compared weakest-tone
thresholds. At +8 dB under this echo, the filtered matcher recovered
0/48 at the current 0.15 requirement and 48/48 at 0.08. This test used
three captures and checked two wrong keys per signal and one noise-only
control. It did not qualify the weaker threshold for live use. The
+15 Hz carrier-offset probe also eliminated setup and level-4 recovery
at +40 dB. The analysis path supplied the inverse offset and clean
alignment and recovered all 24 setup and all 24 level-4 trials.

On the single-VM mixed bench, shared WASAPI playback and a 250 ms
prebuffer enabled exact 203 + 6 + 203 byte exchanges with both Linkwise
roles, including verified disconnect reports from both peers. One
Linkwise-responder run each at +28 dB (20% gain) and +44 dB (original
gain) also passed with AWGN applied to stock-to-Linkwise ingress only.
At 0 dB and 20% gain, one run in each role timed out during connection
after successfully injecting noise. These directional probes do not
estimate a session-success threshold. Current-bridge stock-to-stock,
two impaired directions, repeated trials at each SNR, and live
qualification of the receiver prototype remain open.

### Causal BW500 receiver follow-up

The two-way daemon now detects BW500 bursts in continuous native capture
and in framed PCM. A causal 1110–1890 Hz FIR provides the band power used
for acquisition. Comparing that power with simultaneous out-of-band power
lets the gate close in broadband noise without a pre-calibrated noise
floor. Guarded records with broadband noise also feed the filtered PCM to
the decoders. Quiet records keep the original timing and raw PCM path,
which preserves the level-3 behavior seen at high SNR. Persistent
in-band interference requires separate treatment.

Level-4 setup and DATA use the pilot phase to estimate residual carrier
offset and rotate the differential soft decisions on retry. Levels 1–3
and keyed controls try a bounded offset search and still require the
complete CRC or keyed sequence. The BW500 setup ACK permits a weaker
individual tone while requiring aggregate energy across all 22 tones.
The live bench exposed CRC-valid stock setup frames with the `0x42`
control and level-4 or level-3 requests. The parser accepts that BW500
form with the existing caller checks. For an incoming level-3 request,
the handshake records the requested level and the responder starts DATA
at level 3. The receive matcher recognizes the additional stock
response, confirmation and poll sequences captured at low SNR. Each
variant must match the entire callsign-keyed sequence.

The engineering replay used the same archived captures and SNR
definition as the initial experiment.

| Condition | Exact recoveries | Noise-only decodes |
| --- | ---: | ---: |
| AWGN at 0 dB | 176/176 | 0/176 |
| AWGN at +44 dB | 176/176, including 24/24 level-3 DATA | 0/176 |
| AWGN at 0 and +44 dB, simulated native capture chunks | 88/88 per point | — |
| +15 Hz carrier offset at +8 and +40 dB | 128/128 per point | 0/128 per point |
| 2 ms echo on the setup ACK at +8 and +44 dB | 48/48 per point | 0/48 per point |

The latest sweep emits `streaming_decode` for the modem PCM ingress and
`native_chunks_decode` for the simulated capture and extraction path.
The other replay paths share the current decoders. Use the CSVs retained
from the first study for historical comparisons against main.

To impair both directions in a live mixed run, set both
`--stock-awgn-db` and `--linkwise-awgn-db`, and retain the seed, gain,
record counts, exact payload hashes and both disconnect reports. On
Linkwise transmit, SNR is calibrated in the scheduled PCM before the
Windows playback bridge. After the bridge it can change with the Windows
audio path. Noise is applied to finite ingress or playback records.

A single pass gives an observed working point for that build, route,
payload and seed. A conservative qualification criterion is a one-sided
95% exact binomial lower bound of at least 95% success in each role.
Even with no failures that requires 59 independent trials per role at
one SNR. Use independent seeds and a stable calibrated bridge, and
report any bridge faults separately. A second stock endpoint is needed
to measure a stock-to-stock control on this route.

On the Windows VARA HF 4.9.1 bench, the final receiver completed one
short 13 + 6 + 13 byte session per role at +4 dB with noise injected in
both directions, with identical payload hashes and both disconnect
reports. The build preceding the short-control additions passed the
203 + 6 + 203 byte test at +4 dB in each role. Earlier short probes also
passed in both roles at +8 dB. Some intermediate +4 dB trials failed
during connection. At 0 dB in both directions, the final Linkwise
responder and stock caller both connected and Linkwise received the
first 13 bytes exactly. The six-byte reverse delivery timed out.
Stock-to-Linkwise-only 0 dB runs on an earlier candidate passed in both
roles. All of these tests use a 3 kHz noise reference. The bilateral
tests used a common PCM gain of 0.2. The 95% session threshold remains
unmeasured.
