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

Use `linkwise-responder` to substitute the listener. Select `--bandwidth
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
